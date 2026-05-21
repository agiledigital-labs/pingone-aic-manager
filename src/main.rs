use clap::Parser;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("aic-edit error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> aic_edit::Result<()> {
    if let Err(e) = aic_edit::logging::init() {
        eprintln!("Warning: logging init failed: {e}");
    }

    let cli = aic_edit::cli::Cli::parse();
    if cli.command.is_none() {
        return run_tui().await;
    }
    aic_edit::cli::run(cli).await
}

async fn run_tui() -> aic_edit::Result<()> {
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal).await;
    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> aic_edit::Result<()> {
    let mut app = aic_edit::app::App::new()?;
    app.run(terminal).await
}
