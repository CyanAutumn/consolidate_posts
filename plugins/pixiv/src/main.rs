mod browser_login;
mod client;
mod config;
mod journal;
mod validation;

use upload_plugin_sdk::{run_plugin, Publication, Receipt, Result, UploadContext, UploadDriver};

struct Pixiv;
impl UploadDriver for Pixiv {
    fn id(&self) -> &str {
        "pixiv"
    }
    fn validate(&self, publication: &Publication) -> Vec<String> {
        validation::validate(publication)
    }
    fn upload(&self, publication: &Publication, context: &UploadContext<'_>) -> Result<Receipt> {
        let account_dir = config::account_dir(context)?;
        let config = config::Config::load(&account_dir)?;
        let rating = config.rating(context)?;
        let session = config::read_session(&account_dir)?;
        let client = client::PixivClient::connect(&session, &config.user_id)?;
        journal::upload(&client, publication, context, &config, rating, &account_dir)
    }
}

fn main() -> std::process::ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--diagnose") {
        return browser_login::diagnose();
    }
    if std::env::args().nth(1).as_deref() == Some("--browser-login") {
        return browser_login::main();
    }
    run_plugin(Pixiv)
}
