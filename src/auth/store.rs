use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;

pub trait SecretStore {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}

/// macOS keychain through `/usr/bin/security`.
/// Items are owned by Apple's signed binary, so a rebuilt CLI never triggers an ACL prompt.
pub struct SecurityCli {
    service: String,
}

const NOT_FOUND: i32 = 44;

impl SecurityCli {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_owned() }
    }

    fn command(&self, verb: &str, account: &str) -> Command {
        let mut cmd = Command::new("/usr/bin/security");
        cmd.arg(verb).arg("-a").arg(account).arg("-s").arg(&self.service);
        cmd
    }
}

impl SecretStore for SecurityCli {
    fn get(&self, account: &str) -> Result<Option<String>> {
        let output = self.command("find-generic-password", account).arg("-w").output().context("running security")?;
        if output.status.code() == Some(NOT_FOUND) {
            return Ok(None);
        }
        if !output.status.success() {
            bail!("keychain read failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(Some(String::from_utf8(output.stdout)?.trim_end().to_owned()))
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        let mut child = self
            .command("add-generic-password", account)
            .arg("-U")
            .arg("-w")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("running security")?;
        child.stdin.take().expect("piped stdin").write_all(format!("{secret}\n{secret}\n").as_bytes())?;
        let output = child.wait_with_output()?;
        if !output.status.success() {
            bail!("keychain write failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        let output = self.command("delete-generic-password", account).output().context("running security")?;
        if output.status.success() || output.status.code() == Some(NOT_FOUND) {
            return Ok(());
        }
        bail!("keychain delete failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
}

#[derive(Default)]
pub struct MemoryStore {
    items: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.items.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, secret: &str) -> Result<()> {
        self.items.lock().unwrap().insert(account.to_owned(), secret.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.items.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryStore::default();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "1").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("1"));
        s.set("a", "2").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("2"));
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
    }

    #[test]
    #[ignore = "touches the real login keychain"]
    fn security_cli_round_trip() {
        let s = SecurityCli::new("slack-cli-test");
        s.delete("probe").unwrap();
        assert_eq!(s.get("probe").unwrap(), None);
        s.set("probe", r#"{"token":"one"}"#).unwrap();
        assert_eq!(s.get("probe").unwrap().as_deref(), Some(r#"{"token":"one"}"#));
        s.set("probe", "two").unwrap();
        assert_eq!(s.get("probe").unwrap().as_deref(), Some("two"));
        s.delete("probe").unwrap();
        s.delete("probe").unwrap();
        assert_eq!(s.get("probe").unwrap(), None);
    }
}
