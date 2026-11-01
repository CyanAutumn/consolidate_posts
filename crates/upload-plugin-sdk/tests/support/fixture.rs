use upload_plugin_sdk::*;

struct Fixture;
impl UploadDriver for Fixture {
    fn id(&self) -> &str {
        "fixture"
    }
    fn validate(&self, _: &Publication) -> Vec<String> {
        if std::path::Path::new("reject-validation").exists() {
            vec!["fixture validation rejected".into()]
        } else {
            vec![]
        }
    }
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        if let Some(workspace) = context.workspace {
            std::fs::write(
                std::path::Path::new(&workspace.data_dir).join("workspace.json"),
                serde_json::to_vec(workspace)?,
            )?;
        }
        std::fs::write("started", std::process::id().to_string())?;
        std::fs::write("operation-id", context.operation_id)?;
        if std::path::Path::new("hang").exists() {
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
        if std::path::Path::new("crash").exists() {
            std::process::exit(7);
        }
        if std::path::Path::new("malformed").exists() {
            println!("invalid protocol data");
        }
        if std::path::Path::new("bad-progress").exists() {
            (context.progress)(u64::MAX)?;
        }
        if std::path::Path::new("large-error").exists() {
            return Err(AppError::new("fixture", "x".repeat(MAX_MESSAGE_BYTES + 1)));
        }
        LocalArchive.upload(publication, context)
    }
}
fn main() -> std::process::ExitCode {
    run_plugin(Fixture)
}
