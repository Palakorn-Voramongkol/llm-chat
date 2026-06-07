use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(llm_chat_client::cli::main())
}
