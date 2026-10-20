use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::support;

fn write_config(dir: &Path) -> PathBuf {
    let path = dir.join("xuewen.toml");
    let db = format!("sqlite:{}", dir.join("t.db").display());
    std::fs::write(
        &path,
        format!(
            "inbox_dir = {:?}\nlibrary_root = {:?}\ndatabase_url = {db:?}\n\n[search]\nindex_dir = {:?}\nvector_dir = {:?}\n",
            dir.join("inbox"),
            dir.join("library"),
            dir.join("idx"),
            dir.join("vectors"),
        ),
    )
    .unwrap();
    path
}

fn xuewen(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xuewen"))
        .arg("--config")
        .arg(config)
        .args(args)
        .env_remove("RUST_LOG")
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[tokio::test]
async fn proxy_cookie_set_refuses_a_blank_cookie() {
    let (pool, root) = support::pool_and_root_with_paper("p1").await;
    pool.close().await;
    let config = write_config(root.parent().unwrap());

    assert!(xuewen(&config, &["proxy-cookie", "--set", " a=b "])
        .status
        .success());
    for blank in ["", "   "] {
        let out = xuewen(&config, &["proxy-cookie", "--set", blank]);
        assert!(!out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "Error: empty cookie — to remove the stored cookie, run: xuewen proxy-cookie --clear\n"
        );
    }
    let out = xuewen(&config, &["proxy-cookie"]);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("proxy cookie set (updated "));
}

#[tokio::test]
async fn code_commands_refuse_a_trashed_paper() {
    let (pool, root) = support::pool_and_root_with_paper("cccc3333").await;
    xuewen::library::trash::soft_delete(&pool, "cccc3333")
        .await
        .unwrap();
    let config = write_config(root.parent().unwrap());

    for args in [
        &["code", "set", "cccc", "https://github.com/x/y"][..],
        &["code", "status", "cccc"],
        &["code", "rm", "cccc"],
    ] {
        let out = xuewen(&config, args);
        assert!(!out.status.success(), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            "Error: cccc3333 is in the trash — run: xuewen restore cccc3333\n",
            "{args:?}"
        );
    }
    assert!(xuewen::agent::store::get_paper_code(&pool, "cccc3333")
        .await
        .unwrap()
        .is_none());
}
