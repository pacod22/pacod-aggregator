//! The end-to-end test of a running stack, by default the development stack (`make dev-up`).

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match pacod_agg_testkit::smoke::run(pacod_agg_testkit::smoke::Options::from_env()).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            println!("FAILED: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
