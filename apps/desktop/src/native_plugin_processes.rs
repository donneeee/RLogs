use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::PathBuf,
    process::{Child, Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativePluginLaunch {
    pub plugin_id: String,
    pub package_root: PathBuf,
    pub entrypoint: PathBuf,
    pub data_root: PathBuf,
    pub asset_root: PathBuf,
    pub shared_asset_root: PathBuf,
}

#[derive(Default)]
pub(crate) struct NativePluginProcesses {
    children: BTreeMap<String, Child>,
}

impl NativePluginProcesses {
    pub fn sync(&mut self, launches: Vec<NativePluginLaunch>) -> Result<(), String> {
        let desired = launches
            .into_iter()
            .map(|launch| (launch.plugin_id.clone(), launch))
            .collect::<BTreeMap<_, _>>();
        let desired_ids = desired.keys().cloned().collect::<BTreeSet<_>>();

        let mut stopped = Vec::new();
        for (plugin_id, child) in &mut self.children {
            let exited = child.try_wait().map_err(|error| {
                format!("could not inspect native plug-in {plugin_id}: {error}")
            })?;
            if exited.is_some() || !desired_ids.contains(plugin_id) {
                if exited.is_none() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                stopped.push(plugin_id.clone());
            }
        }
        for plugin_id in stopped {
            self.children.remove(&plugin_id);
        }

        for (plugin_id, launch) in desired {
            if self.children.contains_key(&plugin_id) {
                continue;
            }
            fs::create_dir_all(&launch.data_root).map_err(|error| {
                format!("could not create data folder for native plug-in {plugin_id}: {error}")
            })?;
            fs::create_dir_all(&launch.asset_root).map_err(|error| {
                format!("could not create asset folder for native plug-in {plugin_id}: {error}")
            })?;
            fs::create_dir_all(&launch.shared_asset_root).map_err(|error| {
                format!(
                    "could not create shared asset folder for native plug-in {plugin_id}: {error}"
                )
            })?;
            let stdout = OpenOptions::new()
                .create(true)
                .append(true)
                .open(launch.data_root.join("native-process.stdout.log"))
                .map_err(|error| {
                    format!("could not open stdout log for native plug-in {plugin_id}: {error}")
                })?;
            let stderr = OpenOptions::new()
                .create(true)
                .append(true)
                .open(launch.data_root.join("native-process.stderr.log"))
                .map_err(|error| {
                    format!("could not open stderr log for native plug-in {plugin_id}: {error}")
                })?;

            let mut command = Command::new(&launch.entrypoint);
            command
                .current_dir(&launch.package_root)
                .env("RLOGS_PLUGIN_ID", &plugin_id)
                .env("RLOGS_PLUGIN_ROOT", &launch.package_root)
                .env("RLOGS_PLUGIN_DATA_DIR", &launch.data_root)
                .env("RLOGS_PLUGIN_ASSET_DIR", &launch.asset_root)
                .env("RLOGS_PLUGIN_SHARED_ASSET_DIR", &launch.shared_asset_root)
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            #[cfg(windows)]
            command.creation_flags(CREATE_NO_WINDOW);
            let child = command.spawn().map_err(|error| {
                format!(
                    "could not start native plug-in {plugin_id} at {}: {error}",
                    launch.entrypoint.display()
                )
            })?;
            self.children.insert(plugin_id, child);
        }
        Ok(())
    }
}

impl Drop for NativePluginProcesses {
    fn drop(&mut self) {
        for child in self.children.values_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
