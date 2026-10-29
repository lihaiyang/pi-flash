//! OS 凭据库（keyring）+ `$PF_KEY_*` 变量引用（051-apikey管理.md）。
//!
//! 明文唯一 rest 归宿是系统凭据库（Windows Credential Manager / macOS
//! Keychain，`keyring` crate）；pf-auth.json / models.json 里只放
//! `$PF_KEY_*` 变量名引用，spawn 时在这里解出并注入 pi 子进程环境。
//! auth.json / models.json 都是 pi 的地盘，PF 零写入（仅启动一次性收编）。
/// 凭据库抽象：生产用 [`KeyringVault`]，测试用 [`MemVault`]。
pub trait SecretVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String>;
    fn get(&self, entry: &str) -> Result<Option<String>, String>;
    /// 幂等：条目不存在也算成功。
    fn delete(&self, entry: &str) -> Result<(), String>;
}

/// keyring service 名（凭据管理器里显示的应用名）。
pub const SERVICE: &str = "PiFlash";

/// 真实凭据库。
///
/// - Windows：Credential Manager（`keyring` crate 的 windows-native）。
/// - macOS：**`/usr/bin/security` CLI，不用 keychain 的 Rust 绑定**。
///
/// # 为什么 macOS 绕开 keyring（2026-10-29 实测）
///
/// `keyring` 的 apple-native 走 `SecItemAdd`/`SecItemCopyMatching`，落库的条目
/// 只信任**创建它的那个可执行文件**（adhoc 签名下 designated requirement 就是
/// 该二进制的 cdhash）。而本项目的凭据是**两个二进制**在用：PF 自己（设置页
/// 写/读）和 `pif-secret`（pi 执行 `!'…/pif-secret' get <provider>` 取回）。
/// 实测：PF 写入后 `pif-secret get` 直接 errSecAuthFailed；反过来让
/// `security` 写、`keyring` 读会**卡在系统授权弹窗**上（无人值守环境下挂死）。
///
/// 全链路改走 `/usr/bin/security`（苹果签名、路径稳定）后，条目 ACL 只信任
/// `security`，而写入/读取/删除**三个动作都由它执行**，于是 PF 与 pif-secret
/// 都能无弹窗读写，重建二进制（cdhash 变）也不影响。
///
/// 明文不进 argv：`-w` 后面不带值，`security` 会从 stdin 要两遍密码（原样
/// 回显两行即可），`ps` 里看不到 key。
pub struct KeyringVault;

impl SecretVault for KeyringVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            return macos_keychain::set(SERVICE, entry, secret);
        }
        #[cfg(not(target_os = "macos"))]
        {
            keyring::Entry::new(SERVICE, entry)
                .and_then(|e| e.set_password(secret))
                .map_err(|e| format!("credential vault: {e}"))
        }
    }

    fn get(&self, entry: &str) -> Result<Option<String>, String> {
        #[cfg(target_os = "macos")]
        {
            return macos_keychain::get(SERVICE, entry);
        }
        #[cfg(not(target_os = "macos"))]
        {
            match keyring::Entry::new(SERVICE, entry).and_then(|e| e.get_password()) {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!("credential vault: {e}")),
            }
        }
    }

    fn delete(&self, entry: &str) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            return macos_keychain::delete(SERVICE, entry);
        }
        #[cfg(not(target_os = "macos"))]
        {
            match keyring::Entry::new(SERVICE, entry).and_then(|e| e.delete_credential()) {
                Ok(()) => Ok(()),
                Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(format!("credential vault: {e}")),
            }
        }
    }
}

/// macOS：登录钥匙串的通用密码条目，全部经 `/usr/bin/security`（理由见
/// [`KeyringVault`] 文档）。
#[cfg(target_os = "macos")]
mod macos_keychain {
    use std::io::Write;
    use std::process::{Command, Stdio};

    const SECURITY: &str = "/usr/bin/security";

    pub(super) fn set(service: &str, entry: &str, secret: &str) -> Result<(), String> {
        let mut child = Command::new(SECURITY)
            // -U：同 service/account 已存在则更新（幂等，与 keyring set 同义）
            .args([
                "add-generic-password",
                "-U",
                "-s",
                service,
                "-a",
                entry,
                "-w",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("credential vault: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            // security 会问「password data / retype password」两遍：原样回两次
            let _ = write!(stdin, "{secret}\n{secret}\n");
        }
        let out = child
            .wait_with_output()
            .map_err(|e| format!("credential vault: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "credential vault: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    pub(super) fn get(service: &str, entry: &str) -> Result<Option<String>, String> {
        let out = Command::new(SECURITY)
            .args(["find-generic-password", "-s", service, "-a", entry, "-w"])
            .output()
            .map_err(|e| format!("credential vault: {e}"))?;
        if out.status.success() {
            // -w 只输出密码本体（末尾补一个换行）；其它空白原样保留
            let raw = String::from_utf8_lossy(&out.stdout).into_owned();
            return Ok(Some(raw.trim_end_matches(['\r', '\n']).to_string()));
        }
        // 44 = errSecItemNotFound；老版本只给一行 "could not be found"
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        if out.status.code() == Some(44) || err.contains("could not be found") {
            Ok(None)
        } else {
            Err(format!("credential vault: {}", err.trim()))
        }
    }

    pub(super) fn delete(service: &str, entry: &str) -> Result<(), String> {
        let out = Command::new(SECURITY)
            .args(["delete-generic-password", "-s", service, "-a", entry])
            .output()
            .map_err(|e| format!("credential vault: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let err = String::from_utf8_lossy(&out.stderr).into_owned();
        if out.status.code() == Some(44) || err.contains("could not be found") {
            Ok(()) // 幂等：不存在也算成功（与 keyring 分支同语义）
        } else {
            Err(format!("credential vault: {}", err.trim()))
        }
    }
}

/// 测试用内存实现。
#[derive(Default)]
pub struct MemVault(pub std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl SecretVault for MemVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(entry.into(), secret.into());
        Ok(())
    }

    fn get(&self, entry: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(entry).cloned())
    }

    fn delete(&self, entry: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(entry);
        Ok(())
    }
}

/// 永远失败的实现：驱动降级路径的测试。
pub struct FailVault;

impl SecretVault for FailVault {
    fn set(&self, _entry: &str, _secret: &str) -> Result<(), String> {
        Err("vault down".into())
    }
    fn get(&self, _entry: &str) -> Result<Option<String>, String> {
        Err("vault down".into())
    }
    fn delete(&self, _entry: &str) -> Result<(), String> {
        Err("vault down".into())
    }
}

/// key 值形态（UI 状态文案 + 降级判断）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreMode {
    /// 明文在凭据库，文件里是 `$PF_KEY_*` 引用。
    Vault,
    /// 用户手写的高级引用（`$OTHER` / `!cmd`），PF 不注入。
    Ref,
    /// 凭据库不可用的降级：明文直接在文件里（0600）。
    File,
}

impl StoreMode {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreMode::Vault => "vault",
            StoreMode::Ref => "ref",
            StoreMode::File => "file",
        }
    }

    pub fn from_str(s: &str) -> Option<StoreMode> {
        match s {
            "vault" => Some(StoreMode::Vault),
            "ref" => Some(StoreMode::Ref),
            "file" => Some(StoreMode::File),
            _ => None,
        }
    }
}

/// `$PF_KEY_X` 引用识别 → 变量名（`PF_KEY_X`）。明文 / `$OTHER` / `!cmd` → None。
pub fn pf_ref(value: &str) -> Option<&str> {
    let name = value.strip_prefix('$')?;
    let tail = name.strip_prefix("PF_KEY_")?;
    if tail.is_empty() {
        return None;
    }
    name.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        .then_some(name)
}

/// 账本里的 key 值形态分类。
pub fn classify(value: &str) -> StoreMode {
    if pf_ref(value).is_some() {
        StoreMode::Vault
    } else if value.starts_with('$') || value.starts_with('!') {
        StoreMode::Ref
    } else {
        StoreMode::File
    }
}

/// provider id → `PF_KEY_*` 变量名。字母数字直接大写（deepseek →
/// PF_KEY_DEEPSEEK）；含分隔符时净化并追加原始 id 的 6 位 FNV 短哈希——
/// 否则 `a-b` / `a.b` 净化成同名互相覆盖。
pub fn env_var_name(provider: &str) -> String {
    let base: String = provider
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    let base = base.trim_matches('_');
    let mut name = format!("PF_KEY_{}", if base.is_empty() { "X" } else { base });
    if provider.chars().any(|c| !c.is_ascii_alphanumeric()) {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in provider.as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        name.push_str(&format!("_{:06X}", hash & 0xFFFF_FF));
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pf_ref_shapes() {
        assert_eq!(pf_ref("$PF_KEY_GLM"), Some("PF_KEY_GLM"));
        assert_eq!(pf_ref("$PF_KEY_MY_GW_A1B2C3"), Some("PF_KEY_MY_GW_A1B2C3"));
        assert_eq!(pf_ref("$OTHER_VAR"), None);
        assert_eq!(pf_ref("$PF_KEY_"), None);
        assert_eq!(pf_ref("$PF_KEY_lower"), None);
        assert_eq!(pf_ref("sk-plain"), None);
        assert_eq!(pf_ref("!cmd"), None);
        assert_eq!(pf_ref(""), None);
    }

    #[test]
    fn env_var_names_are_deterministic_and_collision_free() {
        assert_eq!(env_var_name("deepseek"), "PF_KEY_DEEPSEEK");
        assert_eq!(env_var_name("OpenAI"), "PF_KEY_OPENAI");
        assert_eq!(env_var_name(""), "PF_KEY_X");
        // 全分隔符 → 净化为空 + 哈希后缀防撞
        let dashed = env_var_name("---");
        assert!(dashed.starts_with("PF_KEY_X_"), "{dashed}");
        // a-b 与 a.b 净化同名，必须靠哈希区分
        assert_ne!(env_var_name("a-b"), env_var_name("a.b"));
        // 同输入同输出
        assert_eq!(env_var_name("my-gw"), env_var_name("my-gw"));
    }

    #[test]
    fn classify_modes() {
        assert_eq!(classify("$PF_KEY_A"), StoreMode::Vault);
        assert_eq!(classify("$OTHER"), StoreMode::Ref);
        assert_eq!(classify("!cmd"), StoreMode::Ref);
        assert_eq!(classify("sk-plain"), StoreMode::File);
    }
}
