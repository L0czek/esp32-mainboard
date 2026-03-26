use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use defmt_mqtt_decoder::decoder::{DecodedChunk, DefmtStreamDecoder};
use defmt_mqtt_decoder::mqtt::{SubscriberConfig, connect, run_subscription};

#[derive(Parser)]
#[command(version)]
struct Cli {
    #[arg(long, default_value = "broker.local")]
    host: String,

    #[arg(long, default_value_t = 1883)]
    port: u16,

    #[arg(long, default_value = "log/defmt")]
    topic: String,

    #[arg(long, default_value = "defmt-mqtt-decoder")]
    client_id: String,

    #[arg(long, env = "MQTT_USER")]
    username: Option<String>,

    #[arg(long, env = "MQTT_PASSWORD")]
    password: Option<String>,

    #[arg(long, default_value_t = 5)]
    keep_alive_secs: u64,

    #[arg(short, long)]
    elf: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = SubscriberConfig {
        client_id: &cli.client_id,
        host: &cli.host,
        port: cli.port,
        topic: &cli.topic,
        username: cli.username.as_deref(),
        password: cli.password.as_deref(),
        keep_alive_secs: cli.keep_alive_secs,
        request_capacity: 16,
    };

    let mut decoder = ConsolePayloadHandler::new(DefmtStreamDecoder::from_elf(&cli.elf)?);
    let (mut client, mut connection) = connect(&config);
    run_subscription(&mut client, &mut connection, config.topic, &mut decoder)
}

struct ConsolePayloadHandler {
    decoder: DefmtStreamDecoder,
}

impl ConsolePayloadHandler {
    fn new(decoder: DefmtStreamDecoder) -> Self {
        Self { decoder }
    }

    fn write_chunk(&mut self, chunk: DecodedChunk) -> Result<()> {
        let mut stdout = io::stdout().lock();
        let mut stderr = io::stderr().lock();

        for line in chunk.lines {
            writeln!(stdout, "{line}")?;
        }

        for warning in chunk.warnings {
            writeln!(stderr, "{warning}")?;
        }

        Ok(())
    }
}

impl defmt_mqtt_decoder::mqtt::PayloadHandler for ConsolePayloadHandler {
    fn handle_payload(&mut self, payload: &[u8]) -> Result<()> {
        let chunk = self.decoder.decode_chunk(payload)?;
        self.write_chunk(chunk)
    }
}
