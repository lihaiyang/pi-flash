//! `pif-secret`：把系统钥匙库里的 apikey 交给 pi 的小助手。
//!
//! pi 的 `auth.json` 里 PF 只写一行引用：
//! `{ "deepseek": { "type": "api_key", "key": "!'…/pif-secret.exe' get deepseek" } }`
//! pi 解析凭据时会执行该命令、把 stdout 当作 key（`resolveConfigValue` 的
//! `!` 分支）。于是秘密只存在钥匙库，盘上永远没有明文，且**任何前端**
//! （PF / pi TUI / pi-web）都能取到——不依赖 PF 进程在场。
//!
//! 子命令：`get <provider>`（pi 用）/ `set <provider>`（从 stdin 读一行秘密）/
//! `del <provider>`。set/del 是同一把钥匙的维护口：macOS 上钥匙串条目只信任
//! 写它的二进制，所以「写」也必须由本程序做（见 `credentials::KeyringVault`
//! 文档），命令行里顺带留出一个可手工验证/救急的入口。
//!
//! 退出码：0 = 成功；1 = 钥匙库无此条目/不可用；2 = 参数错误。
//! 非 0 时 pi 会判该 provider「未配置」（安全失败，不会把命令串当 key 发出去）。

use pi_link::credentials::{KeyringVault, SecretVault, env_var_name};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || -> ! {
        eprintln!("usage: pif-secret get|set|del <provider>");
        std::process::exit(2);
    };
    if args.len() != 2 {
        usage();
    }
    let provider = &args[1];
    let entry = env_var_name(provider);
    match args[0].as_str() {
        "get" => match KeyringVault.get(&entry) {
            Ok(Some(secret)) if !secret.is_empty() => print!("{secret}"),
            _ => std::process::exit(1),
        },
        "set" => {
            // 秘密从 stdin 读（不进 argv）；末尾换行是管道噪声，去掉
            let mut raw = String::new();
            if std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw).is_err() {
                eprintln!("pif-secret: 读 stdin 失败");
                std::process::exit(1);
            }
            let secret = raw.trim_end_matches(['\r', '\n']);
            if secret.is_empty() {
                eprintln!("pif-secret: stdin 为空");
                std::process::exit(2);
            }
            match KeyringVault.set(&entry, secret) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("pif-secret: {e}");
                    std::process::exit(1);
                }
            }
        }
        "del" => match KeyringVault.delete(&entry) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("pif-secret: {e}");
                std::process::exit(1);
            }
        },
        _ => usage(),
    }
}
