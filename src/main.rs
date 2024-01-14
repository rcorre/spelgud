fn main() -> spelgud::Result<()> {
    env_logger::init();
    let (connection, io_threads) = lsp_server::Connection::stdio();
    spelgud::run(connection)?;
    io_threads.join()?;
    Ok(())
}
