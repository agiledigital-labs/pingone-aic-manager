#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("aic error: {e}");
        std::process::exit(e.exit_code());
    }
}

async fn run() -> pingone_aic_manager::Result<()> {
    if let Err(e) = pingone_aic_manager::logging::init() {
        eprintln!("Warning: logging init failed: {e}");
    }

    // Root the process at the project dir + detect any workspace tenant/realm
    // before parsing, so resolved defaults can be baked into `--help`.
    pingone_aic_manager::cli::bootstrap_project_root()?;
    let cli = pingone_aic_manager::cli::parse_with_defaults();
    if cli.command.is_none() {
        return run_tui().await;
    }
    pingone_aic_manager::cli::run(cli).await
}

async fn run_tui() -> pingone_aic_manager::Result<()> {
    let mut terminal = ratatui::init();
    let result = run_app(&mut terminal).await;
    ratatui::restore();
    result
}

async fn run_app(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
) -> pingone_aic_manager::Result<()> {
    let mut app = pingone_aic_manager::app::App::new()?;
    let result = app.run(terminal).await;
    drop(app);
    result
}
