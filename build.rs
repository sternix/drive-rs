use std::process::Command;
use vergen_gitcl::{Emitter, GitclBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ui klasörü ve svelte.config.js değişince yeniden derle
    println!("cargo:rerun-if-changed=ui/src");
    println!("cargo:rerun-if-changed=ui/svelte.config.js");

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir("ui")
        .status()
        .expect("`npm run build` başarısız");

    assert!(status.success(), "Svelte build hatası");

    // Git commit hash ve tarih bilgisini etkinleştir
    let gitcl = GitclBuilder::default()
        .branch(true)
        .commit_timestamp(true)
        .sha(true)
        .build()?;

    // Bu bilgileri cargo'ya ortam değişkeni olarak aktar
    Emitter::default().add_instructions(&gitcl)?.emit()?;

    Ok(())
}
