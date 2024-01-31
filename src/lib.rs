use std::net::SocketAddr;
use std::process::{Command, Stdio};
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Command output was invalid format (non utf-8)")]
    InvalidOutput,

    #[error("Failed to parse exposed ports, is your docker daemon running?")]
    FailedToParsePorts,
}

pub struct Postgres {
    container_id: String,
    socket_addr: SocketAddr,
}

impl Postgres {
    /// Starts a postgres docker container that will accept all connections with a
    /// random port assigned by docker. The container will be stopped and
    /// removed when the guard is dropped.
    ///
    /// Note that we're using sync code here so we'll block the executor - but only for a short moment
    /// as the container will run in the background.
    pub async fn spawn() -> Result<Self, Error> {
        let container_id = run_cmd_to_output(
            "docker run --rm -d -e POSTGRES_HOST_AUTH_METHOD=trust -p 5432 postgres",
        )?;

        let exposed_port =
            run_cmd_to_output(&format!("docker container port {container_id} 5432"))?;
        let socket_addr = parse_exposed_port(&exposed_port)?;

        // TODO: Properly wait for postgres to be ready
        std::thread::sleep(Duration::from_secs_f32(2.0));

        Ok(Postgres {
            container_id,
            socket_addr,
        })
    }

    pub fn socket_addr(&self) -> SocketAddr {
        self.socket_addr
    }

    pub fn address(&self) -> String {
        self.socket_addr.to_string()
    }
}

impl Drop for Postgres {
    fn drop(&mut self) {
        if let Err(err) = run_cmd(&format!("docker stop {}", &self.container_id)) {
            eprintln!("Failed to stop docker container: {}", err);
        }

        // Redundant, but better safe than sorry
        if let Err(err) = run_cmd(&format!("docker rm {}", &self.container_id)) {
            eprintln!("Failed to remove docker container: {}", err);
        }
    }
}

fn run_cmd_to_output(cmd_str: &str) -> Result<String, Error> {
    let args: Vec<_> = cmd_str.split(' ').collect();
    let mut command = Command::new(args[0]);

    for arg in &args[1..] {
        command.arg(arg);
    }

    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());

    let Ok(output) = command.output() else {
        return Ok(String::new());
    };

    let utf = String::from_utf8(output.stdout).map_err(|err| {
        eprintln!("Failed to parse command output: {}", err);
        Error::InvalidOutput
    })?;

    Ok(utf.trim().to_string())
}

fn run_cmd(cmd_str: &str) -> Result<(), Error> {
    run_cmd_to_output(cmd_str)?;

    Ok(())
}

fn parse_exposed_port(s: &str) -> Result<SocketAddr, Error> {
    let parts: Vec<_> = s
        .split_whitespace()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(parts
        .iter()
        .map(|p| {
            p.parse::<SocketAddr>().map_err(|err| {
                eprintln!("Failed to parse socket addr: {}", err);
                Error::FailedToParsePorts
            })
        })
        .next()
        .ok_or(Error::FailedToParsePorts)??)
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    #[test_case("0.0.0.0:55837" => 55837 ; "base case")]
    #[test_case("   0.0.0.0:55837    " => 55837 ; "ignore whitespace")]
    #[test_case("[::]:12345" => 12345 ; "works with ipv6")]
    #[test_case("0.0.0.0:12345 \n [::]:12345" => 12345 ; "works with multiple ips")]
    #[test_case("0.0.0.0:12345 \n [::]:54321" => 12345 ; "yields first of multiple ips")]
    fn test_parse_exposed_port(s: &str) -> u16 {
        parse_exposed_port(s).unwrap().port()
    }

    #[tokio::test]
    async fn test_spawn() {
        let _ = Postgres::spawn().await.unwrap();
    }
}
