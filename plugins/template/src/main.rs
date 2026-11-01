use upload_plugin_sdk::{
    run_plugin, LocalArchive, Publication, Receipt, Result, UploadContext, UploadDriver,
};

struct ExamplePlugin;
impl UploadDriver for ExamplePlugin {
    fn id(&self) -> &str {
        "example-uploader"
    }

    fn validate(&self, _publication: &Publication) -> Vec<String> {
        // Add platform-specific checks here. Validation must have no publishing side effects.
        Vec::new()
    }

    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        // Working example: export archived copies and a manifest to the account's output folder.
        // Replace this with the platform API implementation. Never report success before it succeeds.
        // context.target identifies the account; publication.assets contain archived file paths.
        // context.workspace contains the workspace ID, content rating and isolated data_dir.
        // Store account configuration under workspace.data_dir / target.id, never this plugin folder.
        // context.operation_id stays stable across retries; use it for remote submission journals.
        LocalArchive.upload(publication, context)
    }
}

// Every process plugin has this one entry. The SDK handles validate/upload JSON, progress and errors.
fn main() -> std::process::ExitCode {
    run_plugin(ExamplePlugin)
}
