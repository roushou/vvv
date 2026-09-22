mod cli;
mod context;
mod output;

use crate::cli::Cli;

fn main() {
    let cli = Cli::parse();

    let (format, color) = (cli.output_format(), cli.color());

    if let Err(err) = cli.run() {
        // The reader went away (`vvv search … | head`): not an error.
        let broken_pipe = err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe);
        if broken_pipe {
            return;
        }
        format.reporter(color, false, false).error(&err);
        std::process::exit(1);
    }
}
