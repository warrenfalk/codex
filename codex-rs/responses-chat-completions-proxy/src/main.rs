use clap::Parser;
use codex_responses_chat_completions_proxy::Args;

#[ctor::ctor]
fn pre_main() {
    codex_process_hardening::pre_main_hardening();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let gemini = args.gemini_generate_content;
    let config = args.into_config()?;
    if gemini {
        codex_responses_chat_completions_proxy::serve_gemini(config).await
    } else {
        codex_responses_chat_completions_proxy::serve(config).await
    }
}
