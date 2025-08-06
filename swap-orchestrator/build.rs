use anyhow::Result;
use vergen::EmitBuilder;

fn main() -> Result<()> {
    EmitBuilder::builder()
        .git_describe(true, true, None)
        .git_sha(false) // false = short hash, true = full hash
        .emit()?;
    Ok(())
}
