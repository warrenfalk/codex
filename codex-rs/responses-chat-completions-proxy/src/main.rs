use clap::Parser;
use codex_responses_chat_completions_proxy::Args;

#[ctor::ctor]
fn pre_main() {
    codex_process_hardening::pre_main_hardening();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Args::parse().into_config()?;
    codex_responses_chat_completions_proxy::serve(config).await
}
