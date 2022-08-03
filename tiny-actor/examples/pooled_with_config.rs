use tiny_actor::*;
use std::time::Duration;
use futures::stream::StreamExt;

// First we define 2 messages:
// SayHi does not return anything, ...
#[derive(Message, Debug)]
struct U32(u32);

// Now we can define a protocol that accepts both messages.
#[protocol]
#[derive(Debug)]
enum MyProtocol {
    Msg(U32),
}

#[tokio::main]
async fn main() {
    let (pool, address) = spawn_many(
        0..3,
        Config {
            link: Link::Attached(Duration::from_secs(1)),
            capacity: Capacity::Unbounded(BackPressure {
                start_at: 5,
                timeout: Duration::from_nanos(25),
                growth: Growth::Exponential(1.3),
            }),
        },
        |i, mut inbox: Inbox<MyProtocol>| async move {
            loop {
                match inbox.recv().await {
                    Ok(msg) => println!("Received message on Actor {i}: {msg:?}"),
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

    tokio::time::sleep(Duration::from_millis(10)).await;

    for num in 0..10 {
        address.send(U32(num)).await.unwrap()
    }

    pool.halt();
    let exits = pool.collect::<Vec<_>>().await;

    for exit in exits {
        match exit {
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
}