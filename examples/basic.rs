use std::time::Duration;
use tiny_actor::*;

#[tokio::main]
async fn main() {
    // First we spawn an actor with a default config, and an inbox which receives u32 messages.
    let (child, address) = spawn(Config::default(), |mut inbox: Inbox<u32>| async move {
        loop {
            // This loops and receives messages
            match inbox.recv().await {
                Ok(msg) => println!("Received message: {msg}"),
                Err(error) => match error {
                    RecvError::Halted => {
                        println!("actor has received halt signal - Exiting now...");
                        break "Halt";
                    }
                    RecvError::ClosedAndEmpty => {
                        println!("Channel is closed - Exiting now...");
                        break "Closed";
                    }
                },
            }
        }
    });

    // Then we can send it messages
    address.send(10).await.unwrap();
    address.send(5).await.unwrap();

    tokio::time::sleep(Duration::from_millis(10)).await;

    // And finally halt the actor for a graceful exit.
    child.halt();
    match child.await {
        Ok(exit) => {
            assert_eq!(exit, "Halt");
            println!("actor exited with message: {exit}")
        }
        Err(error) => match error {
            ExitError::Panic(_) => todo!(),
            ExitError::Abort => todo!(),
        },
    }
}
