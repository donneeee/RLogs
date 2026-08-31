//! Bounded, read-only receipt for exact IL2CPP string slots.
//!
//! This research tool reads only eight reviewed `GameAssembly.dll` globals and
//! the managed `Il2CppString` objects to which they resolve. It does not scan or
//! dump the heap, inject code, patch the client, synthesize packet evidence, or
//! run in any shipped capture/runtime path. Unresolved metadata tokens are
//! retained explicitly and never interpreted as plaintext.

#[cfg(windows)]
mod windows {
    use std::{
        collections::BTreeMap,
        env,
        error::Error,
        ffi::{OsString, c_void},
        fs::{self, File},
        io::{self, Read},
        mem::size_of,
        os::windows::ffi::OsStringExt,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use serde::Serialize;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, MODULEENTRY32W, Module32FirstW, Module32NextW,
                    PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPMODULE,
                    TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
                },
            },
            Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ},
        },
    };

    const EXPECTED_BUILD: &str = "24687926";
    const EXPECTED_ASSEMBLY_BYTES: u64 = 217_629_232;
    const EXPECTED_ASSEMBLY_SHA256: &str =
        "4ba9e3f194bfd1769e57e3f12d192208e4d34db04374636738dfc9d5525495a4";
    const MAXIMUM_STRING_CODE_UNITS: usize = 256;
    const MAXIMUM_USER_ADDRESS: usize = 0x0000_7fff_ffff_ffff;

    #[derive(Debug, Clone, Copy)]
    struct SlotSpec {
        id: &'static str,
        rva: usize,
        structural_role: &'static str,
    }

    const SLOTS: [SlotSpec; 8] = [
        SlotSpec {
            id: "standard-hitdata-offset-0x24-key",
            rva: 0x960_23d0,
            structural_role: "standard parser float lookup stored directly at HitData+0x24",
        },
        SlotSpec {
            id: "standard-hitdata-offset-0x2c-key",
            rva: 0x955_4d08,
            structural_role: "standard parser float lookup divided by LogicFrameRate and stored at HitData+0x2c",
        },
        SlotSpec {
            id: "standard-hitdata-offset-0x30-key",
            rva: 0x94d_5520,
            structural_role: "standard parser integer lookup stored at HitData+0x30 before zero-to-one normalization",
        },
        SlotSpec {
            id: "shared-hitdata-offset-0x34-key",
            rva: 0x955_4e60,
            structural_role: "standard and common parser float lookup divided by LogicFrameRate and stored at HitData+0x34",
        },
        SlotSpec {
            id: "common-hitdata-offset-0x24-key",
            rva: 0x955_4dc8,
            structural_role: "common parser float lookup divided by LogicFrameRate and stored at HitData+0x24",
        },
        SlotSpec {
            id: "common-hitdata-offset-0x28-key",
            rva: 0x955_4df8,
            structural_role: "common parser float lookup divided by LogicFrameRate and stored at HitData+0x28",
        },
        SlotSpec {
            id: "common-hitdata-offset-0x98-key",
            rva: 0x955_4e08,
            structural_role: "common parser optional positive integer lookup stored at HitData+0x98",
        },
        SlotSpec {
            id: "numeric-event-type-key-control",
            rva: 0x955_4e28,
            structural_role: "runtime event-dictionary numeric ESkillEventType grouping control",
        },
    ];

    #[derive(Debug, Serialize)]
    struct ArtifactIdentity {
        path: String,
        byte_length: u64,
        sha256: String,
    }

    #[derive(Debug, Serialize)]
    struct SlotReceipt {
        id: &'static str,
        rva_hex: String,
        structural_role: &'static str,
        slot_address_hex: String,
        raw_slot_value_hex: String,
        state: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_usage_tag: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_index: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        resolved_pointer_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        utf16_code_units_hex: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    #[derive(Debug, Serialize)]
    struct Receipt {
        schema_version: u16,
        generated_by: &'static str,
        game: &'static str,
        deployment: &'static str,
        channel: &'static str,
        game_build: &'static str,
        observed_unix_millis: u128,
        process_id: u32,
        process_name: Option<String>,
        game_assembly_module_base_hex: String,
        game_assembly: ArtifactIdentity,
        slots: Vec<SlotReceipt>,
        summary: Summary,
        policy: Policy,
    }

    #[derive(Debug, Serialize)]
    struct Summary {
        requested_slots: usize,
        resolved_strings: usize,
        unresolved_metadata_tokens: usize,
        read_errors: usize,
        all_requested_strings_resolved: bool,
        parser_lookup_global_to_catalog_parameter_identity_proven: bool,
        provider_rdps_credit_allowed: bool,
        ui_rdps_display_allowed: bool,
        runtime_promotion_allowed: bool,
    }

    #[derive(Debug, Serialize)]
    struct Policy {
        exact_build_identity_required: bool,
        process_access_is_read_only: bool,
        exact_slots_only: bool,
        heap_or_process_scan_performed: bool,
        code_injected_or_patched: bool,
        unresolved_tokens_treated_as_plaintext: bool,
        localized_names_are_runtime_keys: bool,
        receipt_alone_authorizes_attribution: bool,
    }

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    pub fn main() -> Result<(), Box<dyn Error>> {
        let options = parse_options(env::args().skip(1))?;
        let build = required(&options, "build")?;
        if build != EXPECTED_BUILD {
            return Err(format!("this receipt supports exact build {EXPECTED_BUILD}").into());
        }
        let output = PathBuf::from(required(&options, "output")?);
        if output.exists() {
            return Err(format!("refusing to overwrite {}", output.display()).into());
        }
        let assembly_path = PathBuf::from(required(&options, "game-assembly")?);
        let assembly = artifact_identity(&assembly_path)?;
        if assembly.byte_length != EXPECTED_ASSEMBLY_BYTES
            || assembly.sha256 != EXPECTED_ASSEMBLY_SHA256
        {
            return Err("GameAssembly identity does not match exact build 24687926".into());
        }

        let (process_id, process_name) = resolve_process(&options)?;
        let module = find_module(process_id, "GameAssembly.dll")?;
        if !same_path(&module.path, &assembly_path)? {
            return Err(format!(
                "running GameAssembly path {} does not match requested {}",
                module.path.display(),
                assembly_path.display()
            )
            .into());
        }
        let handle =
            unsafe { OpenProcess(PROCESS_VM_READ | PROCESS_QUERY_INFORMATION, 0, process_id) };
        if handle.is_null() {
            return Err(io::Error::last_os_error().into());
        }
        let handle = OwnedHandle(handle);
        let slots = SLOTS
            .iter()
            .map(|spec| read_slot(handle.0, module.base, *spec))
            .collect::<Vec<_>>();
        let resolved_strings = slots
            .iter()
            .filter(|slot| slot.state == "resolved_managed_string")
            .count();
        let unresolved_metadata_tokens = slots
            .iter()
            .filter(|slot| slot.state == "unresolved_metadata_token")
            .count();
        let read_errors = slots
            .iter()
            .filter(|slot| slot.state == "read_error")
            .count();
        let all_requested_strings_resolved = resolved_strings == SLOTS.len();
        let receipt = Receipt {
            schema_version: 1,
            generated_by: "rlogs-bpsr-il2cpp-resolved-string-receipt",
            game: "blue-protocol-star-resonance",
            deployment: "global",
            channel: "steam",
            game_build: EXPECTED_BUILD,
            observed_unix_millis: SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
            process_id,
            process_name,
            game_assembly_module_base_hex: format!("0x{:X}", module.base),
            game_assembly: assembly,
            slots,
            summary: Summary {
                requested_slots: SLOTS.len(),
                resolved_strings,
                unresolved_metadata_tokens,
                read_errors,
                all_requested_strings_resolved,
                parser_lookup_global_to_catalog_parameter_identity_proven: false,
                provider_rdps_credit_allowed: false,
                ui_rdps_display_allowed: false,
                runtime_promotion_allowed: false,
            },
            policy: Policy {
                exact_build_identity_required: true,
                process_access_is_read_only: true,
                exact_slots_only: true,
                heap_or_process_scan_performed: false,
                code_injected_or_patched: false,
                unresolved_tokens_treated_as_plaintext: false,
                localized_names_are_runtime_keys: false,
                receipt_alone_authorizes_attribution: false,
            },
        };
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let encoded = serde_json::to_vec_pretty(&receipt)?;
        fs::write(&output, [&encoded[..], b"\n"].concat())?;
        println!(
            "read {} exact slots: {} resolved, {} unresolved tokens, {} errors; provider credit=false",
            SLOTS.len(),
            resolved_strings,
            unresolved_metadata_tokens,
            read_errors
        );
        println!("wrote {}", output.display());
        Ok(())
    }

    fn read_slot(handle: HANDLE, module_base: usize, spec: SlotSpec) -> SlotReceipt {
        let address = module_base.saturating_add(spec.rva);
        let base = |state, raw, error| SlotReceipt {
            id: spec.id,
            rva_hex: format!("0x{:X}", spec.rva),
            structural_role: spec.structural_role,
            slot_address_hex: format!("0x{address:X}"),
            raw_slot_value_hex: format!("0x{raw:016X}"),
            state,
            metadata_usage_tag: None,
            metadata_index: None,
            resolved_pointer_hex: None,
            utf16_code_units_hex: None,
            value: None,
            error,
        };
        let raw = match read_exact_process(handle, address, size_of::<usize>()) {
            Ok(bytes) => usize::from_le_bytes(bytes.try_into().expect("pointer width")),
            Err(error) => return base("read_error", 0, Some(error.to_string())),
        };
        if raw & 1 == 1 {
            let mut receipt = base("unresolved_metadata_token", raw, None);
            receipt.metadata_usage_tag = Some((raw as u64) >> 29);
            receipt.metadata_index = Some(((raw as u64) >> 1) & 0x0fff_ffff);
            return receipt;
        }
        if !(0x1_0000..=MAXIMUM_USER_ADDRESS).contains(&raw) {
            return base(
                "read_error",
                raw,
                Some("resolved slot does not contain a plausible user-space pointer".into()),
            );
        }
        match read_il2cpp_string(handle, raw) {
            Ok((units, value)) => {
                let mut receipt = base("resolved_managed_string", raw, None);
                receipt.resolved_pointer_hex = Some(format!("0x{raw:X}"));
                receipt.utf16_code_units_hex = Some(utf16_hex(&units));
                receipt.value = Some(value);
                receipt
            }
            Err(error) => base("read_error", raw, Some(error.to_string())),
        }
    }

    fn read_il2cpp_string(
        handle: HANDLE,
        pointer: usize,
    ) -> Result<(Vec<u16>, String), Box<dyn Error>> {
        let header = read_exact_process(handle, pointer, 0x14)?;
        let length = i32::from_le_bytes(header[0x10..0x14].try_into()?);
        if length < 0 || length as usize > MAXIMUM_STRING_CODE_UNITS {
            return Err(format!("invalid Il2CppString length {length}").into());
        }
        let bytes = read_exact_process(handle, pointer + 0x14, length as usize * 2)?;
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        let value = String::from_utf16(&units)?;
        Ok((units, value))
    }

    fn utf16_hex(units: &[u16]) -> String {
        units
            .iter()
            .map(|unit| format!("{unit:04x}"))
            .collect::<Vec<_>>()
            .join("")
    }

    struct ModuleIdentity {
        base: usize,
        path: PathBuf,
    }

    fn find_module(process_id: u32, expected: &str) -> Result<ModuleIdentity, Box<dyn Error>> {
        let snapshot = unsafe {
            CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, process_id)
        };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        let snapshot = OwnedHandle(snapshot);
        let mut entry = MODULEENTRY32W {
            dwSize: size_of::<MODULEENTRY32W>() as u32,
            ..Default::default()
        };
        let mut has_entry = unsafe { Module32FirstW(snapshot.0, &mut entry) } != 0;
        while has_entry {
            let name = wide_string(&entry.szModule);
            if process_names_match(expected, &name) {
                return Ok(ModuleIdentity {
                    base: entry.modBaseAddr as usize,
                    path: PathBuf::from(wide_string(&entry.szExePath)),
                });
            }
            has_entry = unsafe { Module32NextW(snapshot.0, &mut entry) } != 0;
        }
        Err(format!("running process has no {expected} module").into())
    }

    fn resolve_process(
        options: &BTreeMap<String, String>,
    ) -> Result<(u32, Option<String>), Box<dyn Error>> {
        match (options.get("pid"), options.get("process-name")) {
            (Some(pid), None) => Ok((pid.parse()?, None)),
            (None, Some(expected)) => {
                let matches = process_ids_named(expected)?;
                match matches.as_slice() {
                    [] => Err(format!("no running process matched {expected:?}").into()),
                    [(pid, name)] => Ok((*pid, Some(name.clone()))),
                    _ => Err(format!("multiple running processes matched {expected:?}").into()),
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
        let snapshot = OwnedHandle(snapshot);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut matches = Vec::new();
        let mut has_entry = unsafe { Process32FirstW(snapshot.0, &mut entry) } != 0;
        while has_entry {
            let name = wide_string(&entry.szExeFile);
            if process_names_match(expected, &name) {
                matches.push((entry.th32ProcessID, name));
            }
            has_entry = unsafe { Process32NextW(snapshot.0, &mut entry) } != 0;
        }
        Ok(matches)
    }

    fn wide_string(value: &[u16]) -> String {
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        OsString::from_wide(&value[..length])
            .to_string_lossy()
            .into_owned()
    }

    fn process_names_match(expected: &str, actual: &str) -> bool {
        fn normalized(value: &str) -> String {
            let lower = value.to_ascii_lowercase();
            lower.strip_suffix(".exe").unwrap_or(&lower).to_owned()
        }
        normalized(expected) == normalized(actual)
    }

    fn artifact_identity(path: &Path) -> Result<ArtifactIdentity, Box<dyn Error>> {
        let mut source = File::open(path)?;
        let byte_length = source.metadata()?.len();
        let mut digest = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let read = source.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            digest.update(&buffer[..read]);
        }
        Ok(ArtifactIdentity {
            path: absolute_path(path)?.display().to_string(),
            byte_length,
            sha256: format!("{:x}", digest.finalize()),
        })
    }

    fn same_path(left: &Path, right: &Path) -> io::Result<bool> {
        Ok(fs::canonicalize(left)?
            .to_string_lossy()
            .eq_ignore_ascii_case(&fs::canonicalize(right)?.to_string_lossy()))
    }

    fn read_exact_process(
        handle: HANDLE,
        address: usize,
        length: usize,
    ) -> Result<Vec<u8>, Box<dyn Error>> {
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
        if succeeded == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if read != length {
            return Err(format!("read {read} of {length} requested bytes").into());
        }
        Ok(bytes)
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
        fn process_name_matching_is_case_insensitive_and_exe_optional() {
            assert!(process_names_match("BPSR_STEAM", "bpsr_steam.exe"));
            assert!(process_names_match("gameassembly.dll", "GameAssembly.dll"));
            assert!(!process_names_match("BPSR_STEAM", "another.exe"));
        }

        #[test]
        fn utf16_receipt_is_fixed_width_and_lossless() {
            let units = "damageInterval".encode_utf16().collect::<Vec<_>>();
            assert_eq!(
                utf16_hex(&units),
                "00640061006d0061006700650049006e00740065007200760061006c"
            );
            assert_eq!(String::from_utf16(&units).unwrap(), "damageInterval");
        }

        #[test]
        fn exact_slots_cover_standard_common_and_control_keys_without_duplicates() {
            let rvas = SLOTS
                .iter()
                .map(|slot| slot.rva)
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(rvas.len(), SLOTS.len());
            assert!(SLOTS.iter().any(|slot| slot.rva == 0x955_4e60));
            assert!(SLOTS.iter().any(|slot| slot.rva == 0x955_4e28));
        }
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::main()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("rlogs-bpsr-il2cpp-resolved-string-receipt requires Windows");
    std::process::exit(1);
}
