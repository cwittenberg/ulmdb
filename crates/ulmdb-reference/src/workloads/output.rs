//! New-directory publication and content-hash manifest construction.

use super::Workload;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

impl Workload {
    pub fn write_new(&self, path: &Path) -> Result<String, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::create_dir(path)
            .map_err(|e| format!("create new workload directory {}: {e}", path.display()))?;
        let mut manifest = format!(
            "schema = 1\ngenerator = \"splitmix64-bit-oracle-v2\"\nseed = {}\nprefixes = {}\nkv = {}\nqueries = {}\nprefix_profile = \"{}\"\nqualification = false\n",
            self.config.seed,
            self.prefixes.0.len(),
            self.kv.len(),
            self.queries.len(),
            self.config.prefix_profile.name()
        );
        for (name, bytes) in self.encoded() {
            let hash = format!("{:x}", Sha256::digest(&bytes));
            fs::write(path.join(&name), &bytes).map_err(|e| e.to_string())?;
            manifest.push_str(&format!(
                "\n[[files]]\nname = \"{name}\"\nbytes = {}\nsha256 = \"{hash}\"\n",
                bytes.len()
            ));
        }
        fs::write(path.join("manifest.toml"), &manifest).map_err(|e| e.to_string())?;
        Ok(manifest)
    }
}
