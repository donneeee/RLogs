//! Offline, bounded IL2CPP metadata recovery for build-update research.
//!
//! The installed BPSR client currently leaves `global-metadata.dat` empty on
//! disk. This tool reads only committed readable process-memory regions,
//! searches for the IL2CPP metadata magic, validates the complete metadata
//! header, and writes exactly one validated metadata image. It is not used by
//! capture, decoding, live combat reduction, or any shipped plug-in path.

#[cfg(windows)]
mod windows {
    use std::{
        collections::BTreeMap,
        env,
        error::Error,
        ffi::{OsString, c_void},
        fs, io,
        mem::size_of,
        os::windows::ffi::OsStringExt,
        path::{Path, PathBuf},
        time::Instant,
    };

    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                    TH32CS_SNAPPROCESS,
                },
            },
            Memory::{
                MEM_COMMIT, MEM_MAPPED, MEM_PRIVATE, MEMORY_BASIC_INFORMATION, PAGE_GUARD,
                PAGE_NOACCESS, VirtualQueryEx,
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    const METADATA_MAGIC: [u8; 4] = [0xaf, 0x1b, 0xb1, 0xfa];
    const HEADER_LENGTH: usize = 0x180;
    const MAXIMUM_USER_ADDRESS: usize = 0x0000_7fff_ffff_ffff;
    const MAXIMUM_METADATA_LENGTH: usize = 0x4000_0000;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum RegionScope {
        Private,
        PrivateAndMapped,
    }

    #[derive(Debug, Clone, Copy)]
    struct MetadataCandidate {
        address: usize,
        version: i32,
        length: usize,
        plausible_pairs: usize,
        region_base: usize,
        region_size: usize,
        region_type: u32,
    }

    #[derive(Debug, Clone, Serialize)]
    struct FileIdentity {
        path: String,
        byte_length: u64,
        sha256: String,
    }

    #[derive(Debug, Serialize)]
    struct ScanReport {
        process_id: u32,
        process_name: Option<String>,
        metadata_address: String,
        metadata_version: i32,
        metadata_length: usize,
        metadata_sha256: String,
        plausible_header_pairs: usize,
        output_path: String,
        game_assembly: Option<FileIdentity>,
        scope: &'static str,
        region_scope: &'static str,
        chunk_mib: usize,
        queried_regions: u64,
        scanned_regions: u64,
        scanned_bytes: u64,
        elapsed_ms: u128,
    }

    #[derive(Debug, Serialize)]
    struct BuildIdentityReport {
        schema_version: u16,
        generated_by: &'static str,
        game: &'static str,
        deployment: String,
        channel: String,
        game_build: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        distribution_app_id: Option<String>,
        metadata: ArtifactDigest,
        game_assembly: ArtifactDigest,
    }

    #[derive(Debug, Serialize)]
    struct ArtifactDigest {
        byte_length: u64,
        sha256: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_version: Option<i32>,
    }

    struct ProcessHandle(HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    pub fn main() -> Result<(), Box<dyn Error>> {
        let options = parse_options(env::args().skip(1))?;
        let (process_id, process_name) = resolve_process(&options)?;
        let output = PathBuf::from(required(&options, "output")?);
        let chunk_mib = options
            .get("chunk-mib")
            .map(String::as_str)
            .unwrap_or("8")
            .parse::<usize>()?;
        if !(1..=64).contains(&chunk_mib) {
            return Err("--chunk-mib must be between 1 and 64".into());
        }
        let region_scope = match options.get("scope").map(String::as_str) {
            None | Some("private") => RegionScope::Private,
            Some("private-and-mapped") => RegionScope::PrivateAndMapped,
            Some(_) => return Err("--scope must be private or private-and-mapped".into()),
        };

        let handle =
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, process_id) };
        if handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let handle = ProcessHandle(handle);
        let started = Instant::now();
        let mut candidates = Vec::new();
        let mut queried_regions = 0u64;
        let mut scanned_regions = 0u64;
        let mut scanned_bytes = 0u64;
        let chunk_size = chunk_mib * 1024 * 1024;
        let mut address = 0usize;

        while address < MAXIMUM_USER_ADDRESS {
            let mut region = MEMORY_BASIC_INFORMATION::default();
            let queried = unsafe {
                VirtualQueryEx(
                    handle.0,
                    address as *const c_void,
                    &mut region,
                    size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if queried == 0 {
                break;
            }
            queried_regions += 1;
            let region_base = region.BaseAddress as usize;
            let region_end = region_base.saturating_add(region.RegionSize);

            if readable_region(&region, region_scope) && region.RegionSize >= 0x10000 {
                scanned_regions += 1;
                scan_region(
                    handle.0,
                    region_base,
                    region_end,
                    chunk_size,
                    &mut scanned_bytes,
                    &mut candidates,
                    &region,
                );
            }

            if region_end <= address {
                break;
            }
            address = region_end;
        }

        candidates.sort_by_key(|candidate| candidate.address);
        candidates.dedup_by_key(|candidate| candidate.address);
        if candidates.len() != 1 {
            let summary: Vec<_> = candidates
                .iter()
                .map(|candidate| {
                    serde_json::json!({
                        "address": format!("0x{:X}", candidate.address),
                        "version": candidate.version,
                        "length": candidate.length,
                        "plausible_pairs": candidate.plausible_pairs,
                        "region_base": format!("0x{:X}", candidate.region_base),
                        "region_size": candidate.region_size,
                        "region_type": candidate.region_type,
                    })
                })
                .collect();
            return Err(format!(
                "expected exactly one validated IL2CPP metadata image, found {}: {}",
                candidates.len(),
                serde_json::to_string(&summary)?
            )
            .into());
        }

        let candidate = candidates[0];
        let bytes = read_exact_process(handle.0, candidate.address, candidate.length)?;
        let final_header = validate_metadata_header(&bytes[..HEADER_LENGTH]);
        if final_header.is_none_or(|header| {
            header.version != candidate.version
                || header.length != candidate.length
                || header.plausible_pairs != candidate.plausible_pairs
        }) {
            return Err("metadata header changed between discovery and final read".into());
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let metadata_sha256 = sha256_bytes(&bytes);
        fs::write(&output, bytes)?;
        let game_assembly = options
            .get("game-assembly")
            .map(|path| file_identity(Path::new(path)))
            .transpose()?;
        if options.contains_key("identity-report") && game_assembly.is_none() {
            return Err("--identity-report requires --game-assembly".into());
        }
        let report = ScanReport {
            process_id,
            process_name,
            metadata_address: format!("0x{:X}", candidate.address),
            metadata_version: candidate.version,
            metadata_length: candidate.length,
            metadata_sha256,
            plausible_header_pairs: candidate.plausible_pairs,
            output_path: absolute_path(&output)?.display().to_string(),
            game_assembly: game_assembly.clone(),
            scope: "validated IL2CPP metadata image only; no heap or process dump",
            region_scope: match region_scope {
                RegionScope::Private => "private",
                RegionScope::PrivateAndMapped => "private-and-mapped",
            },
            chunk_mib,
            queried_regions,
            scanned_regions,
            scanned_bytes,
            elapsed_ms: started.elapsed().as_millis(),
        };
        let report_json = serde_json::to_string_pretty(&report)?;
        if let Some(report_path) = options.get("report") {
            let report_path = Path::new(report_path);
            if let Some(parent) = report_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(report_path, report_json.as_bytes())?;
        }
        if let Some(identity_path) = options.get("identity-report") {
            let deployment = required(&options, "deployment")?.to_owned();
            let channel = required(&options, "channel")?.to_owned();
            let (game_build, distribution_app_id) = resolve_build_identity(&options)?;
            for (name, value) in [
                ("deployment", deployment.as_str()),
                ("channel", channel.as_str()),
                ("build", game_build.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(format!("--{name} must not be empty").into());
                }
            }
            let assembly = game_assembly
                .as_ref()
                .expect("--identity-report requires a validated assembly identity");
            let identity = BuildIdentityReport {
                schema_version: 1,
                generated_by: "rlogs-bpsr-il2cpp-metadata-scan",
                game: "blue-protocol-star-resonance",
                deployment,
                channel,
                game_build,
                distribution_app_id,
                metadata: ArtifactDigest {
                    byte_length: candidate.length as u64,
                    sha256: report.metadata_sha256.clone(),
                    metadata_version: Some(candidate.version),
                },
                game_assembly: ArtifactDigest {
                    byte_length: assembly.byte_length,
                    sha256: assembly.sha256.clone(),
                    metadata_version: None,
                },
            };
            let identity_json = serde_json::to_string_pretty(&identity)?;
            let identity_path = Path::new(identity_path);
            if let Some(parent) = identity_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(identity_path, identity_json.as_bytes())?;
        }
        println!("{report_json}");
        Ok(())
    }

    fn resolve_build_identity(
        options: &BTreeMap<String, String>,
    ) -> Result<(String, Option<String>), Box<dyn Error>> {
        let manifest_identity = options
            .get("steam-manifest")
            .map(|path| {
                let contents = fs::read_to_string(path)?;
                let build = acf_value(&contents, "buildid")
                    .ok_or("Steam manifest does not contain buildid")?;
                let app_id = acf_value(&contents, "appid");
                Ok::<_, Box<dyn Error>>((build, app_id))
            })
            .transpose()?;
        match (options.get("build"), manifest_identity) {
            (Some(explicit), Some((manifest, _app_id))) if explicit != &manifest => Err(format!(
                "--build {explicit:?} does not match Steam manifest buildid {manifest:?}"
            )
            .into()),
            (Some(explicit), Some((_, app_id))) => Ok((explicit.clone(), app_id)),
            (Some(explicit), None) => Ok((explicit.clone(), None)),
            (None, Some(identity)) => Ok(identity),
            (None, None) => Err("--identity-report requires --build or --steam-manifest".into()),
        }
    }

    fn acf_value(contents: &str, key: &str) -> Option<String> {
        contents.lines().find_map(|line| {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let actual_key = quoted.next()?;
            let value = quoted.next()?;
            actual_key
                .eq_ignore_ascii_case(key)
                .then(|| value.to_owned())
        })
    }

    fn resolve_process(
        options: &BTreeMap<String, String>,
    ) -> Result<(u32, Option<String>), Box<dyn Error>> {
        match (options.get("pid"), options.get("process-name")) {
            (Some(pid), None) => Ok((pid.parse::<u32>()?, None)),
            (None, Some(process_name)) => {
                let matches = process_ids_named(process_name)?;
                match matches.as_slice() {
                    [] => Err(format!("no running process matched {process_name:?}").into()),
                    [(pid, actual_name)] => Ok((*pid, Some(actual_name.clone()))),
                    _ => Err(format!(
                        "more than one running process matched {process_name:?}: {}",
                        serde_json::to_string(&matches)?
                    )
                    .into()),
                }
            }
            (Some(_), Some(_)) => Err("use either --pid or --process-name, not both".into()),
            (None, None) => Err("missing --pid or --process-name".into()),
        }
    }

    fn process_ids_named(expected: &str) -> Result<Vec<(u32, String)>, Box<dyn Error>> {
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let snapshot = ProcessHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut matches = Vec::new();
        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while has_entry {
            let length = entry
                .szExeFile
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(entry.szExeFile.len());
            let actual_name = OsString::from_wide(&entry.szExeFile[..length])
                .to_string_lossy()
                .into_owned();
            if process_names_match(expected, &actual_name) {
                matches.push((entry.th32ProcessID, actual_name));
            }
            has_entry = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(matches)
    }

    fn process_names_match(expected: &str, actual: &str) -> bool {
        fn normalized(value: &str) -> String {
            let lower = value.to_ascii_lowercase();
            lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
        }
        normalized(expected) == normalized(actual)
    }

    fn file_identity(path: &Path) -> Result<FileIdentity, Box<dyn Error>> {
        let bytes = fs::read(path)?;
        Ok(FileIdentity {
            path: absolute_path(path)?.display().to_string(),
            byte_length: bytes.len() as u64,
            sha256: sha256_bytes(&bytes),
        })
    }

    fn sha256_bytes(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn scan_region(
        handle: HANDLE,
        region_base: usize,
        region_end: usize,
        chunk_size: usize,
        scanned_bytes: &mut u64,
        candidates: &mut Vec<MetadataCandidate>,
        region: &MEMORY_BASIC_INFORMATION,
    ) {
        let mut cursor = region_base;
        let mut overlap = Vec::<u8>::new();
        while cursor < region_end {
            let request = chunk_size.min(region_end - cursor);
            let Some(chunk) = read_process(handle, cursor, request) else {
                break;
            };
            if chunk.is_empty() {
                break;
            }
            *scanned_bytes = scanned_bytes.saturating_add(chunk.len() as u64);

            let mut scan = Vec::with_capacity(overlap.len() + chunk.len());
            scan.extend_from_slice(&overlap);
            scan.extend_from_slice(&chunk);
            let scan_base = cursor.saturating_sub(overlap.len());
            for index in magic_offsets(&scan) {
                let candidate_address = scan_base + index;
                if candidates
                    .iter()
                    .any(|candidate| candidate.address == candidate_address)
                {
                    continue;
                }
                let Some(header) = read_process(handle, candidate_address, HEADER_LENGTH) else {
                    continue;
                };
                let Some(mut candidate) = validate_metadata_header(&header) else {
                    continue;
                };
                candidate.address = candidate_address;
                candidate.region_base = region_base;
                candidate.region_size = region.RegionSize;
                candidate.region_type = region.Type;
                candidates.push(candidate);
            }

            overlap.clear();
            overlap.extend_from_slice(&chunk[chunk.len().saturating_sub(3)..]);
            cursor = cursor.saturating_add(chunk.len());
            if chunk.len() < request {
                break;
            }
        }
    }

    fn readable_region(region: &MEMORY_BASIC_INFORMATION, scope: RegionScope) -> bool {
        region.State == MEM_COMMIT
            && region.Protect & PAGE_NOACCESS == 0
            && region.Protect & PAGE_GUARD == 0
            && (region.Type == MEM_PRIVATE
                || (scope == RegionScope::PrivateAndMapped && region.Type == MEM_MAPPED))
    }

    fn magic_offsets(bytes: &[u8]) -> impl Iterator<Item = usize> + '_ {
        bytes
            .windows(METADATA_MAGIC.len())
            .enumerate()
            .filter_map(|(index, window)| (window == METADATA_MAGIC).then_some(index))
    }

    fn validate_metadata_header(bytes: &[u8]) -> Option<MetadataCandidate> {
        if bytes.len() < HEADER_LENGTH || read_u32(bytes, 0)? != 0xfab1_1baf {
            return None;
        }
        let version = read_i32(bytes, 4)?;
        if !(20..=40).contains(&version) {
            return None;
        }
        let mut maximum_end = 0usize;
        let mut plausible_pairs = 0usize;
        for offset in (8..=0x178).step_by(8) {
            let table_offset = read_u32(bytes, offset)? as usize;
            let table_size = read_u32(bytes, offset + 4)? as usize;
            if table_offset == 0 && table_size == 0 {
                continue;
            }
            if table_offset < 0x80
                || table_offset > MAXIMUM_METADATA_LENGTH
                || table_size > MAXIMUM_METADATA_LENGTH
            {
                continue;
            }
            let end = table_offset.checked_add(table_size)?;
            if end > MAXIMUM_METADATA_LENGTH {
                continue;
            }
            plausible_pairs += 1;
            maximum_end = maximum_end.max(end);
        }
        if plausible_pairs < 12 || maximum_end < 0x10000 {
            return None;
        }
        Some(MetadataCandidate {
            address: 0,
            version,
            length: maximum_end,
            plausible_pairs,
            region_base: 0,
            region_size: 0,
            region_type: 0,
        })
    }

    fn read_process(handle: HANDLE, address: usize, length: usize) -> Option<Vec<u8>> {
        let mut bytes = vec![0u8; length];
        let mut read = 0usize;
        let succeeded = unsafe {
            ReadProcessMemory(
                handle,
                address as *const c_void,
                bytes.as_mut_ptr().cast(),
                length,
                &mut read,
            )
        };
        if succeeded == 0 || read == 0 {
            return None;
        }
        bytes.truncate(read);
        Some(bytes)
    }

    fn read_exact_process(
        handle: HANDLE,
        address: usize,
        length: usize,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
        let bytes =
            read_process(handle, address, length).ok_or_else(|| io::Error::last_os_error())?;
        if bytes.len() != length {
            return Err(format!(
                "metadata candidate was found but only {} of {length} bytes could be read",
                bytes.len()
            )
            .into());
        }
        Ok(bytes)
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
        Some(u32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    }

    fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
        Some(i32::from_le_bytes(
            bytes.get(offset..offset + 4)?.try_into().ok()?,
        ))
    }

    fn parse_options(
        mut arguments: impl Iterator<Item = String>,
    ) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
        let mut options = BTreeMap::new();
        while let Some(argument) = arguments.next() {
            let key = argument
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected positional argument: {argument}"))?;
            let value = arguments
                .next()
                .ok_or_else(|| format!("missing value for --{key}"))?;
            if options.insert(key.to_owned(), value).is_some() {
                return Err(format!("duplicate option --{key}").into());
            }
        }
        Ok(options)
    }

    fn required<'a>(
        options: &'a BTreeMap<String, String>,
        key: &str,
    ) -> Result<&'a str, Box<dyn Error>> {
        options
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("missing --{key}").into())
    }

    fn absolute_path(path: &Path) -> io::Result<PathBuf> {
        if path.is_absolute() {
            Ok(path.to_owned())
        } else {
            Ok(env::current_dir()?.join(path))
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn validates_a_plausible_metadata_header_and_rejects_noise() {
            let mut header = vec![0u8; HEADER_LENGTH];
            header[0..4].copy_from_slice(&0xfab1_1bafu32.to_le_bytes());
            header[4..8].copy_from_slice(&31i32.to_le_bytes());
            for (index, offset) in (8..=0x178).step_by(8).take(16).enumerate() {
                let table_offset = 0x1000u32 + index as u32 * 0x1000;
                header[offset..offset + 4].copy_from_slice(&table_offset.to_le_bytes());
                header[offset + 4..offset + 8].copy_from_slice(&0x1000u32.to_le_bytes());
            }
            let candidate = validate_metadata_header(&header).expect("valid header");
            assert_eq!(candidate.version, 31);
            assert_eq!(candidate.length, 0x11000);
            assert_eq!(candidate.plausible_pairs, 16);

            header[0] = 0;
            assert!(validate_metadata_header(&header).is_none());
        }

        #[test]
        fn finds_magic_at_chunk_boundaries() {
            let bytes = [0, 0xaf, 0x1b, 0xb1, 0xfa, 1];
            assert_eq!(magic_offsets(&bytes).collect::<Vec<_>>(), vec![1]);
        }

        #[test]
        fn process_name_matching_is_case_insensitive_and_exe_optional() {
            assert!(process_names_match("BPSR_STEAM", "bpsr_steam.exe"));
            assert!(process_names_match("bpsr_steam.exe", "BPSR_STEAM.EXE"));
            assert!(!process_names_match("BPSR_STEAM", "another.exe"));
        }

        #[test]
        fn hashes_bytes_deterministically() {
            assert_eq!(
                sha256_bytes(b"rLogs"),
                "c7cf57e976ff0ad7e9da6a349d7975d61e726bc7ba84d6cb9ce068392ed17b7d"
            );
        }

        #[test]
        fn reads_exact_steam_manifest_keys_without_confusing_target_build() {
            let manifest = r#"
                "appid" "3681810"
                "buildid" "24568685"
                "TargetBuildID" "99999999"
            "#;
            assert_eq!(acf_value(manifest, "appid").as_deref(), Some("3681810"));
            assert_eq!(acf_value(manifest, "buildid").as_deref(), Some("24568685"));
        }
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::main()
}

#[cfg(not(windows))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err("IL2CPP process-memory metadata recovery is available only on Windows; generated artifacts remain portable and are consumed by the cross-platform BPSR plug-in".into())
}
