use std::time::Duration;
use tiny_actor::*;

// First we define 2 messages:
// SayHi does not return anything, ...
#[derive(Message, Debug)]
struct SayHi;

// while Echo returns a String.
#[derive(Message, Debug)]
#[reply(String)]
struct Echo(String);

// Now we can define a protocol that accepts both messages.
#[protocol]
#[derive(Debug)]
enum MyProtocol {
    One(SayHi),
    Two(Echo),
}

#[tokio::main]
async fn main() {
    // Spawn an process with a default config, and an Inbox<MyProtocol>
    let (child, address) = spawn(
        Config::default(),
        |mut inbox: Inbox<MyProtocol>| async move {
            // Now constantly loop and receive a message from the inbox
            loop {
                match inbox.recv().await {
                    Ok(msg) => match msg {
                        MyProtocol::One(SayHi, ()) => {
                            println!("Received SayHi message!")
                        }
                        MyProtocol::Two(Echo(string), tx) => {
                            println!("Echoing message: {string}");
                            let _ = tx.send(string);
                        }
                    },
                    Err(error) => match error {
                        RecvError::Halted => {
                            println!("Actor has received halt signal - Exiting now...");
                            break "Halt";
                        }
                        RecvError::ClosedAndEmpty => {
                            println!("Actor is closed - Exiting now...");
                            break "Closed";
                        }
                    },
                }
            }
        },
    );

    // Send the SayHi message
    let () = address.send(SayHi).await.unwrap();

    // Send and then receive the Echo message
    let rx: Rx<String> = address.send(Echo("hi".to_string())).await.unwrap();
    let _reply: String = rx.await.unwrap();

    // Which can be shortened to:
    let _reply: String = address.send(Echo("hi".to_string())).recv().await.unwrap();

    // Wait for the message to arrive
    tokio::time::sleep(Duration::from_millis(10)).await;

    // And halt the actor
    child.halt();

    // Now we can await the child to see how it has exited
    match child.await {
        Ok(exit) => {
            assert_eq!(exit, "Halt");
            println!("Actor exited with message: {exit}")
        }
        Err(error) => match error {
            ExitError::Panic(_) => todo!(),
            ExitError::Abort => todo!(),
        },
    }
}
