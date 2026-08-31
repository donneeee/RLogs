using System.Security.Cryptography;
using System.Text.Json;
using MemoryPack;

const int SchemaVersion = 4;
const string GeneratedBy = "tools/bpsr-skill-logic-decoder";
const string ExpectedBuild = "24687926";
const string ExpectedPayloadSha256 = "c49a1de1d2eb38943df2586142267999b5b6f0616951ce457811822ff72bc18c";

if (args.Length == 0)
{
    Fail("usage: analyze --build <id> --payload <bin> --output <json> | verify --input <json>");
}

var values = new List<string>(args);
var command = values[0];
values.RemoveAt(0);
if (command == "analyze")
{
    var build = Take(values, "--build");
    var payloadPath = Path.GetFullPath(Take(values, "--payload"));
    var outputPath = Path.GetFullPath(Take(values, "--output"));
    if (values.Count != 0) Fail($"unknown arguments: {string.Join(' ', values)}");
    if (build != ExpectedBuild) Fail($"this reviewed decoder supports only build {ExpectedBuild}");
    if (File.Exists(outputPath)) Fail($"refusing to overwrite existing output: {outputPath}");
    var payload = File.ReadAllBytes(payloadPath);
    if (Sha256(payload) != ExpectedPayloadSha256) Fail("payload is not the reviewed exact current build");
    var decoded = MemoryPackSerializer.Deserialize<ZLogicDataSourceTotal>(payload)
        ?? throw new InvalidDataException("MemoryPack returned a null ZLogicDataSourceTotal");
    var report = BuildReport(payloadPath, payload, decoded);
    Validate(report);
    File.WriteAllText(
        outputPath,
        JsonSerializer.Serialize(report, JsonOptions()) + Environment.NewLine,
        System.Text.Encoding.UTF8);
    Console.WriteLine($"wrote exact-build stage logic catalog: {outputPath}");
    Console.WriteLine($"skill keys: {report.Summary.SkillDictionaryKeys:N0}; stage rows: {report.Summary.SkillStageRows:N0}");
}
else if (command == "verify")
{
    var inputPath = Path.GetFullPath(Take(values, "--input"));
    if (values.Count != 0) Fail($"unknown arguments: {string.Join(' ', values)}");
    var report = JsonSerializer.Deserialize<StageCatalogReport>(File.ReadAllText(inputPath), JsonOptions())
        ?? throw new InvalidDataException("stage catalog JSON is null");
    Validate(report);
    Console.WriteLine($"verified exact-build stage logic catalog: {inputPath}");
}
else
{
    Fail($"unknown command {command}");
}

return;

static string Take(List<string> values, string flag)
{
    var index = values.IndexOf(flag);
    if (index < 0 || index + 1 >= values.Count) Fail($"{flag} requires a value");
    var result = values[index + 1];
    values.RemoveRange(index, 2);
    return result;
}

static void Fail(string message) => throw new InvalidOperationException(message);

static string Sha256(byte[] bytes) => Convert.ToHexStringLower(SHA256.HashData(bytes));

static JsonSerializerOptions JsonOptions() => new()
{
    PropertyNamingPolicy = JsonNamingPolicy.SnakeCaseLower,
    WriteIndented = true,
};

static StageCatalogReport BuildReport(string payloadPath, byte[] payload, ZLogicDataSourceTotal decoded)
{
    var skillRows = Rows(decoded.SkillDict, "skill");
    var bulletRows = Rows(decoded.BulletDict, "bullet");
    var buffRows = Rows(decoded.BuffDict, "buff");
    var behaviorRows = Behaviors(decoded);
    var triggerRows = Triggers(decoded);
    var conditionRows = Conditions(decoded);
    var eventParameterRows = EventParameters(decoded.EventParamDict);
    var stageEventRows = StageEvents(decoded);
    var eventParameterKeys = eventParameterRows.Select(row => row.ParameterIndex).ToHashSet();
    var unresolvedStageEventParameterReferences = stageEventRows
        .SelectMany(row => row.ParameterIndexes)
        .LongCount(index => !eventParameterKeys.Contains(index));
    var duplicateStageIds = skillRows
        .GroupBy(row => (row.DictionaryKey, row.StageId))
        .Where(group => group.Count() > 1)
        .Select(group => new DuplicateStageId(group.Key.DictionaryKey, group.Key.StageId, group.Count()))
        .OrderBy(row => row.DictionaryKey)
        .ThenBy(row => row.StageId)
        .ToArray();
    var indexEqualsStageId = skillRows.Count(row => row.StageIndex == row.StageId);
    return new StageCatalogReport(
        SchemaVersion,
        GeneratedBy,
        "blue-protocol-star-resonance",
        "global",
        "steam",
        ExpectedBuild,
        new Artifact(payloadPath, payload.Length, Sha256(payload)),
        new MemberOrderEvidence(
            new[] { "SkillDict", "BulletDict", "BuffDict", "EventParamDict" },
            new[] { "SkillId", "StageEventList", "StageLogicList" },
            new[] { "StageId", "StageType", "StageTriggerList" },
            "exact current-build IL2CPP field order; MemoryPack object layout is sequential"),
        new CatalogSummary(
            decoded.SkillDict?.Count ?? 0,
            decoded.BulletDict?.Count ?? 0,
            decoded.BuffDict?.Count ?? 0,
            decoded.EventParamDict?.Count ?? 0,
            skillRows.Length,
            bulletRows.Length,
            buffRows.Length,
            skillRows.Select(row => row.StageType).Distinct().Order().ToArray(),
            skillRows.GroupBy(row => row.StageType).OrderBy(group => group.Key)
                .Select(group => new ValueCount(group.Key, group.LongCount())).ToArray(),
            behaviorRows.LongLength,
            behaviorRows.GroupBy(row => row.BehaveType).OrderBy(group => group.Key)
                .Select(group => new ValueCount(group.Key, group.LongCount())).ToArray(),
            triggerRows.LongLength,
            conditionRows.LongLength,
            eventParameterRows.LongLength,
            stageEventRows.LongLength,
            stageEventRows.Sum(row => (long)row.ParameterIndexes.Length),
            unresolvedStageEventParameterReferences,
            indexEqualsStageId,
            skillRows.Length - indexEqualsStageId,
            duplicateStageIds.Length),
        skillRows,
        bulletRows,
        buffRows,
        behaviorRows,
        triggerRows,
        conditionRows,
        eventParameterRows,
        stageEventRows,
        duplicateStageIds,
        new Authority(
            true,
            true,
            false,
            false,
            false,
            false),
        new[]
        {
            "join packet owner action to the exact SkillDict key through reviewed SkillTable EffectIDs/SkillLevelGroup routing",
            "interpret packet owner_stage only as the zero-based StageLogicList index after the action-to-SkillDict-key join",
            "prove provider-removed speed ordering, integer timing/damage projection, and party-damage conservation",
        });
}

static StageRow[] Rows(Dictionary<long, ZLogicDataSource>? dictionary, string dictionaryKind)
{
    if (dictionary is null) return Array.Empty<StageRow>();
    return dictionary
        .OrderBy(pair => pair.Key)
        .SelectMany(pair => (pair.Value.StageLogicList ?? new List<StageLogicInfo>())
            .Select((stage, index) => new StageRow(
                dictionaryKind,
                pair.Key,
                pair.Value.SkillId,
                index,
                stage.StageId,
                stage.StageType,
                stage.StageTriggerList?.Count ?? 0,
                pair.Value.StageEventList?.Count ?? 0)))
        .ToArray();
}

static BehaviorRow[] Behaviors(ZLogicDataSourceTotal decoded)
{
    return new[]
        {
            (Kind: "skill", Dictionary: decoded.SkillDict),
            (Kind: "bullet", Dictionary: decoded.BulletDict),
            (Kind: "buff", Dictionary: decoded.BuffDict),
        }
        .SelectMany(source => (source.Dictionary ?? new Dictionary<long, ZLogicDataSource>())
            .Select(pair => (source.Kind, pair.Key, pair.Value)))
        .OrderBy(row => row.Kind)
        .ThenBy(row => row.Key)
        .SelectMany(source => (source.Value.StageLogicList ?? new List<StageLogicInfo>())
            .SelectMany((stage, stageIndex) => (stage.StageTriggerList ?? new List<StageTrigger>())
                .SelectMany((trigger, triggerIndex) =>
                    (trigger.ConditionGroupList ?? new List<ConditionGroup>())
                    .SelectMany((group, conditionGroupIndex) =>
                        (group.BehaveGroups ?? new List<Behave>())
                        .Select((behave, behaveIndex) => new BehaviorRow(
                            source.Kind,
                            source.Key,
                            source.Value.SkillId,
                            stageIndex,
                            stage.StageId,
                            stage.StageType,
                            triggerIndex,
                            trigger.TriggerIdx,
                            trigger.TriggerType,
                            conditionGroupIndex,
                            group.GroupIdx,
                            group.ServerGroupIdx,
                            behaveIndex,
                            behave.BehaveType,
                            behave.BehaveParams?.ToArray() ?? Array.Empty<double>()))))))
        .OrderBy(row => row.DictionaryKind)
        .ThenBy(row => row.DictionaryKey)
        .ThenBy(row => row.StageIndex)
        .ThenBy(row => row.TriggerIndex)
        .ThenBy(row => row.ConditionGroupIndex)
        .ThenBy(row => row.BehaveIndex)
        .ToArray();
}

static TriggerRow[] Triggers(ZLogicDataSourceTotal decoded)
{
    return LogicDictionaries(decoded)
        .SelectMany(source => (source.Dictionary ?? new Dictionary<long, ZLogicDataSource>())
            .Select(pair => (source.Kind, pair.Key, pair.Value)))
        .OrderBy(row => row.Kind)
        .ThenBy(row => row.Key)
        .SelectMany(source => (source.Value.StageLogicList ?? new List<StageLogicInfo>())
            .SelectMany((stage, stageIndex) => (stage.StageTriggerList ?? new List<StageTrigger>())
                .Select((trigger, triggerIndex) => new TriggerRow(
                    source.Kind,
                    source.Key,
                    source.Value.SkillId,
                    stageIndex,
                    stage.StageId,
                    stage.StageType,
                    triggerIndex,
                    trigger.TriggerIdx,
                    trigger.TriggerType,
                    trigger.TriggerParameter?.ToArray() ?? Array.Empty<double>()))))
        .OrderBy(row => row.DictionaryKind)
        .ThenBy(row => row.DictionaryKey)
        .ThenBy(row => row.StageIndex)
        .ThenBy(row => row.TriggerIndex)
        .ToArray();
}

static ConditionRow[] Conditions(ZLogicDataSourceTotal decoded)
{
    return LogicDictionaries(decoded)
        .SelectMany(source => (source.Dictionary ?? new Dictionary<long, ZLogicDataSource>())
            .Select(pair => (source.Kind, pair.Key, pair.Value)))
        .OrderBy(row => row.Kind)
        .ThenBy(row => row.Key)
        .SelectMany(source => (source.Value.StageLogicList ?? new List<StageLogicInfo>())
            .SelectMany((stage, stageIndex) => (stage.StageTriggerList ?? new List<StageTrigger>())
                .SelectMany((trigger, triggerIndex) =>
                    (trigger.ConditionGroupList ?? new List<ConditionGroup>())
                    .SelectMany((group, conditionGroupIndex) =>
                        (group.Conditions ?? new List<Condition>())
                        .Select((condition, conditionIndex) => new ConditionRow(
                            source.Kind,
                            source.Key,
                            source.Value.SkillId,
                            stageIndex,
                            stage.StageId,
                            stage.StageType,
                            triggerIndex,
                            trigger.TriggerIdx,
                            trigger.TriggerType,
                            conditionGroupIndex,
                            group.GroupIdx,
                            group.ServerGroupIdx,
                            conditionIndex,
                            condition.ConditionType,
                            condition.CompareType,
                            condition.ConditionParams?.ToArray() ?? Array.Empty<double>(),
                            condition.Value))))))
        .OrderBy(row => row.DictionaryKind)
        .ThenBy(row => row.DictionaryKey)
        .ThenBy(row => row.StageIndex)
        .ThenBy(row => row.TriggerIndex)
        .ThenBy(row => row.ConditionGroupIndex)
        .ThenBy(row => row.ConditionIndex)
        .ToArray();
}

static (string Kind, Dictionary<long, ZLogicDataSource>? Dictionary)[] LogicDictionaries(
    ZLogicDataSourceTotal decoded) =>
    new[]
    {
        ("skill", decoded.SkillDict),
        ("bullet", decoded.BulletDict),
        ("buff", decoded.BuffDict),
    };

static EventParameterRow[] EventParameters(Dictionary<int, StageEventParamData>? dictionary)
{
    if (dictionary is null) return Array.Empty<EventParameterRow>();
    return dictionary
        .OrderBy(pair => pair.Key)
        .Select(pair => new EventParameterRow(
            pair.Key,
            pair.Value.ParamName,
            pair.Value.ParamType,
            pair.Value.ParamValue))
        .ToArray();
}

static StageEventRow[] StageEvents(ZLogicDataSourceTotal decoded)
{
    return new[]
        {
            (Kind: "skill", Dictionary: decoded.SkillDict),
            (Kind: "bullet", Dictionary: decoded.BulletDict),
            (Kind: "buff", Dictionary: decoded.BuffDict),
        }
        .SelectMany(source => (source.Dictionary ?? new Dictionary<long, ZLogicDataSource>())
            .Select(pair => (source.Kind, pair.Key, pair.Value)))
        .OrderBy(row => row.Kind)
        .ThenBy(row => row.Key)
        .SelectMany(source => (source.Value.StageEventList ?? new List<StageEventInfo>())
            .Select((stageEvent, eventIndex) => new StageEventRow(
                source.Kind,
                source.Key,
                source.Value.SkillId,
                eventIndex,
                stageEvent.Name,
                stageEvent.StageMaxTime,
                stageEvent.ParamIndexList?.ToArray() ?? Array.Empty<int>())))
        .ToArray();
}

static void Validate(StageCatalogReport report)
{
    if ((report.SchemaVersion < 1 || report.SchemaVersion > SchemaVersion) ||
        report.GeneratedBy != GeneratedBy ||
        report.Game != "blue-protocol-star-resonance" ||
        report.Deployment != "global" ||
        report.Channel != "steam" ||
        report.Build != ExpectedBuild ||
        report.Payload.Sha256 != ExpectedPayloadSha256 ||
        report.Summary.SkillDictionaryKeys <= 0 ||
        report.Summary.SkillStageRows <= 0 ||
        report.SkillStages.Length != report.Summary.SkillStageRows ||
        (report.SchemaVersion >= 2 &&
            (report.BehaviorRows is null ||
             report.Summary.BehaviorTypeCounts is null ||
             report.BehaviorRows.LongLength != report.Summary.BehaviorRows ||
             report.BehaviorRows.LongLength != report.Summary.BehaviorTypeCounts.Sum(row => row.Count))) ||
        (report.SchemaVersion >= 3 &&
            (report.EventParameterRows is null ||
             report.StageEventRows is null ||
             report.EventParameterRows.LongLength != report.Summary.EventParameterRows ||
             report.EventParameterRows.LongLength != report.Summary.EventParameterKeys ||
             report.StageEventRows.LongLength != report.Summary.StageEventRows ||
             report.StageEventRows.Sum(row => (long)row.ParameterIndexes.Length) !=
                report.Summary.StageEventParameterReferences ||
             report.Summary.UnresolvedStageEventParameterReferences != 0)) ||
        (report.SchemaVersion >= 4 &&
            (report.TriggerRows is null ||
             report.ConditionRows is null ||
             report.TriggerRows.LongLength != report.Summary.TriggerRows ||
             report.ConditionRows.LongLength != report.Summary.ConditionRows)) ||
        report.Authority.ExactBuildSkillLogicPayloadDecoded != true ||
        report.Authority.StageLogicMemberOrderProven != true ||
        report.Authority.PacketOwnerStageToStageTypeMappingProven != false ||
        report.Authority.RuntimePromotionAllowed != false ||
        report.Authority.UiDisplayAuthority != false ||
        report.Authority.ProviderRdpsCreditAllowed != false)
    {
        Fail("stage logic catalog is not fail-closed exact-build evidence");
    }
    foreach (var row in report.SkillStages)
    {
        if (row.StageIndex < 0 || row.StageId < 0 || row.StageType < 0)
            Fail("stage logic catalog contains an invalid negative identity");
    }
}

public sealed record Artifact(string File, int Bytes, string Sha256);
public sealed record MemberOrderEvidence(string[] Total, string[] Source, string[] Stage, string Basis);
public sealed record ValueCount(int Value, long Count);
public sealed record DuplicateStageId(long DictionaryKey, int StageId, int Count);
public sealed record CatalogSummary(
    int SkillDictionaryKeys,
    int BulletDictionaryKeys,
    int BuffDictionaryKeys,
    int EventParameterKeys,
    int SkillStageRows,
    int BulletStageRows,
    int BuffStageRows,
    int[] ObservedSkillStageTypes,
    ValueCount[] SkillStageTypeCounts,
    long BehaviorRows,
    ValueCount[] BehaviorTypeCounts,
    long TriggerRows,
    long ConditionRows,
    long EventParameterRows,
    long StageEventRows,
    long StageEventParameterReferences,
    long UnresolvedStageEventParameterReferences,
    int SkillRowsWhereIndexEqualsStageId,
    int SkillRowsWhereIndexDiffersFromStageId,
    int DuplicateSkillStageIdGroups);
public sealed record StageRow(
    string DictionaryKind,
    long DictionaryKey,
    int SourceSkillId,
    int StageIndex,
    int StageId,
    int StageType,
    int StageTriggerCount,
    int StageEventCount);
public sealed record BehaviorRow(
    string DictionaryKind,
    long DictionaryKey,
    int SourceSkillId,
    int StageIndex,
    int StageId,
    int StageType,
    int TriggerIndex,
    int TriggerIdx,
    int TriggerType,
    int ConditionGroupIndex,
    int GroupIdx,
    int ServerGroupIdx,
    int BehaveIndex,
    int BehaveType,
    double[] BehaveParams);
public sealed record TriggerRow(
    string DictionaryKind,
    long DictionaryKey,
    int SourceSkillId,
    int StageIndex,
    int StageId,
    int StageType,
    int TriggerIndex,
    int TriggerIdx,
    int TriggerType,
    double[] TriggerParameters);
public sealed record ConditionRow(
    string DictionaryKind,
    long DictionaryKey,
    int SourceSkillId,
    int StageIndex,
    int StageId,
    int StageType,
    int TriggerIndex,
    int TriggerIdx,
    int TriggerType,
    int ConditionGroupIndex,
    int GroupIdx,
    int ServerGroupIdx,
    int ConditionIndex,
    int ConditionType,
    int CompareType,
    double[] ConditionParameters,
    float Value);
public sealed record EventParameterRow(
    int ParameterIndex,
    string? ParamName,
    string? ParamType,
    string? ParamValue);
public sealed record StageEventRow(
    string DictionaryKind,
    long DictionaryKey,
    int SourceSkillId,
    int EventIndex,
    string? Name,
    float StageMaxTime,
    int[] ParameterIndexes);
public sealed record Authority(
    bool ExactBuildSkillLogicPayloadDecoded,
    bool StageLogicMemberOrderProven,
    bool PacketOwnerStageToStageTypeMappingProven,
    bool RuntimePromotionAllowed,
    bool UiDisplayAuthority,
    bool ProviderRdpsCreditAllowed);
public sealed record StageCatalogReport(
    int SchemaVersion,
    string GeneratedBy,
    string Game,
    string Deployment,
    string Channel,
    string Build,
    Artifact Payload,
    MemberOrderEvidence MemberOrderEvidence,
    CatalogSummary Summary,
    StageRow[] SkillStages,
    StageRow[] BulletStages,
    StageRow[] BuffStages,
    BehaviorRow[] BehaviorRows,
    TriggerRow[]? TriggerRows,
    ConditionRow[]? ConditionRows,
    EventParameterRow[] EventParameterRows,
    StageEventRow[] StageEventRows,
    DuplicateStageId[] DuplicateSkillStageIds,
    Authority Authority,
    string[] NextRequiredProof);

[MemoryPackable]
public partial class ZLogicDataSourceTotal
{
    public Dictionary<long, ZLogicDataSource>? SkillDict;
    public Dictionary<long, ZLogicDataSource>? BulletDict;
    public Dictionary<long, ZLogicDataSource>? BuffDict;
    public Dictionary<int, StageEventParamData>? EventParamDict;
}

[MemoryPackable]
public partial class ZLogicDataSource
{
    public int SkillId;
    public List<StageEventInfo>? StageEventList;
    public List<StageLogicInfo>? StageLogicList;
}

[MemoryPackable]
public partial class StageEventInfo
{
    public string? Name;
    public float StageMaxTime;
    public List<int>? ParamIndexList;
}

[MemoryPackable]
public partial class StageEventParamData
{
    public string? ParamName;
    public string? ParamType;
    public string? ParamValue;
}

[MemoryPackable]
public partial class StageLogicInfo
{
    public int StageId;
    public int StageType;
    public List<StageTrigger>? StageTriggerList;
}

[MemoryPackable]
public partial class StageTrigger
{
    public int TriggerIdx;
    public int TriggerType;
    public List<double>? TriggerParameter;
    public List<ConditionGroup>? ConditionGroupList;
}

[MemoryPackable]
public partial class ConditionGroup
{
    public int GroupIdx;
    public int ServerGroupIdx;
    public List<Behave>? BehaveGroups;
    public List<Condition>? Conditions;
}

[MemoryPackable]
public partial class Behave
{
    public int BehaveType;
    public List<double>? BehaveParams;
}

[MemoryPackable]
public partial class Condition
{
    public int ConditionType;
    public int CompareType;
    public List<double>? ConditionParams;
    public float Value;
}
