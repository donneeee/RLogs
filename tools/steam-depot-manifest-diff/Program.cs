using System.Security.Cryptography;
using System.Globalization;
using System.Text;
using System.Text.Json;
using SteamKit2;

internal static class Program
{
    private static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
        WriteIndented = true,
    };

    private sealed record ManifestEvidence(
        string SourceFile,
        string BinarySha256,
        uint DepotId,
        string ManifestGid,
        DateTime CreationTimeUtc,
        ulong TotalUncompressedBytes,
        ulong TotalCompressedBytes,
        bool FilenamesEncrypted,
        int RecordCount,
        int FileCount,
        int DirectoryCount);

    private sealed record FileRecord(
        string Identity,
        string? Path,
        string FileNameSha1,
        string ContentSha1,
        ulong Size,
        uint Flags,
        int ChunkCount,
        string ChunkFingerprintSha256,
        string? LinkTarget);

    private sealed record ManifestSnapshot(
        string SchemaVersion,
        ManifestEvidence Manifest,
        IReadOnlyList<FileRecord> Files);

    private sealed record ChangedRecord(
        string Identity,
        string? Path,
        IReadOnlyList<string> Reasons,
        FileRecord Previous,
        FileRecord Current);

    private sealed record ManifestDiff(
        string SchemaVersion,
        ManifestEvidence PreviousManifest,
        ManifestEvidence CurrentManifest,
        object Summary,
        IReadOnlyList<FileRecord> Added,
        IReadOnlyList<FileRecord> Removed,
        IReadOnlyList<ChangedRecord> Changed,
        IReadOnlyList<FileRecord> Unchanged);

    private static int Main(string[] args)
    {
        try
        {
            if (args.Length == 0 || args[0] is "-h" or "--help")
            {
                PrintUsage();
                return 0;
            }

            var command = args[0].ToLowerInvariant();
            var options = ParseOptions(args.Skip(1).ToArray());
            return command switch
            {
                "inspect" => Inspect(Require(options, "manifest"), Optional(options, "output")),
                "diff" => Diff(
                    Require(options, "old"),
                    Require(options, "new"),
                    Require(options, "output")),
                _ => throw new ArgumentException($"Unknown command '{args[0]}'."),
            };
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error.Message);
            return 1;
        }
    }

    private static int Inspect(string manifestPath, string? outputPath)
    {
        var snapshot = ReadSnapshot(manifestPath);
        WriteJson(snapshot, outputPath);
        return 0;
    }

    private static int Diff(string oldManifestPath, string newManifestPath, string outputPath)
    {
        var previous = ReadSnapshot(oldManifestPath);
        var current = ReadSnapshot(newManifestPath);
        if (previous.Manifest.DepotId != current.Manifest.DepotId)
        {
            throw new InvalidDataException(
                $"Depot mismatch: {previous.Manifest.DepotId} != {current.Manifest.DepotId}.");
        }

        var oldByIdentity = previous.Files.ToDictionary(file => file.Identity, StringComparer.Ordinal);
        var newByIdentity = current.Files.ToDictionary(file => file.Identity, StringComparer.Ordinal);
        var added = new List<FileRecord>();
        var removed = new List<FileRecord>();
        var changed = new List<ChangedRecord>();
        var unchanged = new List<FileRecord>();

        foreach (var currentFile in current.Files)
        {
            if (!oldByIdentity.TryGetValue(currentFile.Identity, out var previousFile))
            {
                added.Add(currentFile);
                continue;
            }

            var reasons = DifferenceReasons(previousFile, currentFile);
            if (reasons.Count == 0)
            {
                unchanged.Add(currentFile);
            }
            else
            {
                changed.Add(new ChangedRecord(
                    currentFile.Identity,
                    currentFile.Path ?? previousFile.Path,
                    reasons,
                    previousFile,
                    currentFile));
            }
        }

        foreach (var previousFile in previous.Files)
        {
            if (!newByIdentity.ContainsKey(previousFile.Identity))
            {
                removed.Add(previousFile);
            }
        }

        static string SortKey(FileRecord file) => file.Path ?? file.Identity;
        added.Sort((left, right) => StringComparer.Ordinal.Compare(SortKey(left), SortKey(right)));
        removed.Sort((left, right) => StringComparer.Ordinal.Compare(SortKey(left), SortKey(right)));
        changed.Sort((left, right) => StringComparer.Ordinal.Compare(left.Path ?? left.Identity, right.Path ?? right.Identity));
        unchanged.Sort((left, right) => StringComparer.Ordinal.Compare(SortKey(left), SortKey(right)));

        var diff = new ManifestDiff(
            "rlogs.steam-depot-manifest-diff.v1",
            previous.Manifest,
            current.Manifest,
            new
            {
                added = added.Count,
                removed = removed.Count,
                changed = changed.Count,
                unchanged = unchanged.Count,
                candidateFiles = added.Count + removed.Count + changed.Count,
                plaintextPathsAvailable = !previous.Manifest.FilenamesEncrypted && !current.Manifest.FilenamesEncrypted,
            },
            added,
            removed,
            changed,
            unchanged);

        WriteJson(diff, outputPath);
        Console.WriteLine(
            $"Depot {current.Manifest.DepotId}: {added.Count} added, {removed.Count} removed, " +
            $"{changed.Count} changed, {unchanged.Count} unchanged.");
        return 0;
    }

    private static ManifestSnapshot ReadSnapshot(string manifestPath)
    {
        var fullPath = Path.GetFullPath(manifestPath);
        using var stream = File.OpenRead(fullPath);
        var binarySha256 = Convert.ToHexString(SHA256.HashData(stream)).ToLowerInvariant();
        stream.Position = 0;
        var manifest = DepotManifest.Deserialize(stream);
        var files = manifest.Files ?? throw new InvalidDataException("Manifest contains no file records.");
        var records = files.Select(file => ToRecord(file, manifest.FilenamesEncrypted)).ToList();

        var duplicate = records
            .GroupBy(record => record.Identity, StringComparer.Ordinal)
            .FirstOrDefault(group => group.Count() > 1);
        if (duplicate is not null)
        {
            throw new InvalidDataException($"Manifest contains duplicate file identity {duplicate.Key}.");
        }

        records.Sort((left, right) =>
            StringComparer.Ordinal.Compare(left.Path ?? left.Identity, right.Path ?? right.Identity));
        var directoryCount = records.Count(record => IsDirectory(record.Flags));
        var evidence = new ManifestEvidence(
            Path.GetFileName(fullPath),
            binarySha256,
            manifest.DepotID,
            manifest.ManifestGID.ToString(CultureInfo.InvariantCulture),
            manifest.CreationTime.ToUniversalTime(),
            manifest.TotalUncompressedSize,
            manifest.TotalCompressedSize,
            manifest.FilenamesEncrypted,
            records.Count,
            records.Count - directoryCount,
            directoryCount);
        return new ManifestSnapshot("rlogs.steam-depot-manifest-snapshot.v1", evidence, records);
    }

    private static FileRecord ToRecord(DepotManifest.FileData file, bool filenamesEncrypted)
    {
        var fileNameHash = Hex(file.FileNameHash);
        var path = filenamesEncrypted ? null : NormalizePath(file.FileName);
        var identity = fileNameHash.Length > 0 ? fileNameHash : $"path:{path}";
        return new FileRecord(
            identity,
            path,
            fileNameHash,
            Hex(file.FileHash),
            file.TotalSize,
            (uint)file.Flags,
            file.Chunks.Count,
            ChunkFingerprint(file.Chunks),
            string.IsNullOrEmpty(file.LinkTarget) ? null : NormalizePath(file.LinkTarget));
    }

    private static IReadOnlyList<string> DifferenceReasons(FileRecord previous, FileRecord current)
    {
        var reasons = new List<string>();
        if (!StringComparer.Ordinal.Equals(previous.ContentSha1, current.ContentSha1)) reasons.Add("content-sha1");
        if (previous.Size != current.Size) reasons.Add("size");
        if (previous.Flags != current.Flags) reasons.Add("flags");
        if (previous.ChunkCount != current.ChunkCount) reasons.Add("chunk-count");
        if (!StringComparer.Ordinal.Equals(previous.ChunkFingerprintSha256, current.ChunkFingerprintSha256)) reasons.Add("chunks");
        if (!StringComparer.Ordinal.Equals(previous.LinkTarget, current.LinkTarget)) reasons.Add("link-target");
        if (!StringComparer.Ordinal.Equals(previous.Path, current.Path)) reasons.Add("path");
        return reasons;
    }

    private static string ChunkFingerprint(IReadOnlyList<DepotManifest.ChunkData> chunks)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        Span<byte> integer = stackalloc byte[8];
        foreach (var chunk in chunks.OrderBy(chunk => chunk.Offset))
        {
            hash.AppendData(chunk.ChunkID ?? []);
            BitConverter.TryWriteBytes(integer, chunk.Offset);
            hash.AppendData(integer);
            BitConverter.TryWriteBytes(integer, (ulong)chunk.CompressedLength);
            hash.AppendData(integer);
            BitConverter.TryWriteBytes(integer, (ulong)chunk.UncompressedLength);
            hash.AppendData(integer);
            BitConverter.TryWriteBytes(integer, (ulong)chunk.Checksum);
            hash.AppendData(integer);
        }
        return Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant();
    }

    private static bool IsDirectory(uint flags) => (flags & 64U) != 0;
    private static string NormalizePath(string path) => path.Replace('\\', '/').TrimStart('/');
    private static string Hex(byte[]? bytes) => bytes is { Length: > 0 }
        ? Convert.ToHexString(bytes).ToLowerInvariant()
        : string.Empty;

    private static Dictionary<string, string> ParseOptions(string[] args)
    {
        var options = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        for (var index = 0; index < args.Length; index++)
        {
            var name = args[index];
            if (!name.StartsWith("--", StringComparison.Ordinal) || index + 1 >= args.Length)
            {
                throw new ArgumentException($"Expected --name value, found '{name}'.");
            }
            options[name[2..]] = args[++index];
        }
        return options;
    }

    private static string Require(IReadOnlyDictionary<string, string> options, string name) =>
        options.TryGetValue(name, out var value)
            ? value
            : throw new ArgumentException($"Missing required --{name} option.");

    private static string? Optional(IReadOnlyDictionary<string, string> options, string name) =>
        options.TryGetValue(name, out var value) ? value : null;

    private static void WriteJson<T>(T value, string? outputPath)
    {
        var json = JsonSerializer.Serialize(value, JsonOptions) + Environment.NewLine;
        if (string.IsNullOrWhiteSpace(outputPath))
        {
            Console.Write(json);
            return;
        }

        var fullPath = Path.GetFullPath(outputPath);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        File.WriteAllText(fullPath, json, new UTF8Encoding(false));
    }

    private static void PrintUsage()
    {
        Console.WriteLine("SteamDepotManifestDiff");
        Console.WriteLine("  inspect --manifest <cached.manifest> [--output <snapshot.json>]");
        Console.WriteLine("  diff --old <cached.manifest> --new <cached.manifest> --output <diff.json>");
    }
}
