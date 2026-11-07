//! Native log output for the Rune entry point.
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};

struct Output(File);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(bytes);
        self.0.write_all(bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        self.0.flush()
    }
}

pub fn init(path: &Path, verbose: bool) {
    let filter = if verbose {
        "warn,navifast=debug"
    } else {
        "warn,navifast=info"
    };
    let mut logger =
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter));
    let file = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| File::create(path));
    match file {
        Ok(file) => {
            logger.target(env_logger::Target::Pipe(Box::new(Output(file))));
        }
        Err(error) => eprintln!("unable to open the log file: {error}"),
    }
    logger.format(|buf, record| {
        writeln!(
            buf,
            "[{} {} {}] {}",
            buf.timestamp(),
            record.level(),
            record.target(),
            record.args()
        )
    });
    if logger.try_init().is_ok() {
        log::info!(
            "Starting navifast {} on {} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
}
