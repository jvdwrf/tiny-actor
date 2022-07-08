# tiny-actor
Tiny-actor is a minimal actor framework for Rust. Because it tries to stay as minimal as possible,
it can be used both in libraries, as well as in applications.

The core priniciple of tiny-actor is the mergin of inboxes and processes. It's impossible
to create an inbox without a process. This allows for building simple pools and supervision-trees, with
reliable shutdown behaviour.

# Concepts

## Channel
A channel is that which underlies the coupling of inboxes, addresses and children. A channel contains: 
* One `Child` or `ChildGroup`
* Zero or more `Address`es
* Zero or more `Inbox`es

## Inbox
An `Inbox` refers is the receiver-part of a `Channel`, coupled with a `tokio::task`. This inbox is primarily used
to take messages out of the `Channel`. `Inbox`es can only be created by spawning new processes. In general,
`Inboxes` should stay coupled to the `tokio::task`, and not be sent to other processes. An `Inbox` should only
be dropped when the `tokio::task` is exiting.

## Address
An address is the cloneable sender-part of a `Channel`. This address is primarily used to send messages to
inboxes. When all `Address`es are dropped, the `Channel` is closed automatically. `Address`es can be awaited, 
which will return when all `Inbox`es linked to the `Channel` have `exit`ed.

## Child
A `Child` is a handle to a `Channel` with a single `Inbox`. This child can be awaited to return the exit-value
of the process that has been spawned. A `Child` is non-cloneable, and therefore unique to the `Channel`. When
the `Child` is dropped, by default the attached process will be `abort`-ed. This can be prevented by `detach`-ing 
the `Child`. More processes can be spawned onto a `Child`, this will turn the `Child` into a `Child-Group`.

## ChildGroup
A `ChildGroup` is similar to a `Child`, except that this is a handle to multiple processes. A `ChildGroup` can be
streamed to return the exit-values of all spawned processes. More processes can be spawned onto a `ChildGroup`.

## Closing
When a `Channel` is `Closed`, it is not longer possible to send new messages into it.
It is still possible to take out any messages that are remaining. A channel that is closed does not
have to exit afterwards. Any senders are notified with a `SendError::Closed`, receivers will receive
`RecvError::ClosedAndEmpty` once the channel has been emptied.

## Halting
An `Inbox` can be `halt`ed exactly once, when `halt`ed, the process should `exit`.
When an `Inbox` is `halt`ed, the `Inbox` will receive a `RecvError::Halted`.
A `Channel` can be partially `halt`ed, which means that only some of the `Inbox`es will receive a `halt`-signal.

## Aborting
A process can be aborted through tokio's `abort()` method. This will stop the process the first time it calls
`.await` anywhere. Therefore, this causes the process to exit abruptly, and can leave bad state behind.
By default, a spawned process is automatically aborted if the `Child` is dropped. This can be prevented by
`detach`-ing a `Child`, later it can be `attach`-ed again.

## Exiting
Exit can refer to two seperate events which, with good practise, always occur at the same time:
* An `Inbox` can exit by being dropped. Once all `Inboxes` of a `Channel` have been dropped, the `Channel` itself
has exited. This type of exit can be retrieved/awaited from the `Channel` at any time.
* A `tokio::task` can exit, which means the process is no longer alive. This can only be queried only once, by 
awaiting the `Child` or `ChildGroup`.

Therefore, it is recommended to drop an `Inbox` only when the process itself is also exiting. This way, an exit 
always refers to the same event.

## Abort-timer
A `Child` or `ChildGroup` has an abort-timer. If the `Child` or `ChildGroup` is attached, then it will instantly
send a `Halt`-signal to all inboxes. Then, after the abort-timer, if the child still has not exited, it will be
aborted.

# Examples

## Basic
```rust
use tiny_actor::*;

#[tokio::main]
async fn main() {
    let (child, address) = spawn(Config::unbounded(), |mut inbox: Inbox<u32>| async move {
        loop {
            match inbox.recv().await {
                Ok(msg) => println!("Received message: {msg}"),
                Err(error) => match error {
                    RecvError::Halted => {
                        println!("Actor has received halt signal - Exiting now...");
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

    address.send(10).await.unwrap();
    address.send(5).await.unwrap();

    child.halt();

    match child.await {
        Ok(exit) => {
            assert_eq!(exit, "Halt");
            println!("Actor exited with message: {exit}")
        },
        Err(error) => match error {
            JoinError::Panic(_) => todo!(),
            JoinError::Abort => todo!(),
        },
    }
}
```

## Pooled with config
```rust
use tiny_actor::*;
use std::time::Duration;
use futures::stream::StreamExt;

#[tokio::main]
async fn main() {
    let (pool, address) = spawn_pooled(
        0..3,
        Config {
            abort_timer: Duration::from_secs(1),
            attached: true,
            capacity: Some(5),
        },
        |i, mut inbox: Inbox<u32>| async move {
            loop {
                match inbox.recv().await {
                    Ok(msg) => println!("Received message on actor {i}: {msg}"),
                    Err(error) => match error {
                        RecvError::Halted => {
                            println!("Actor has received halt signal - Exiting now...");
                            break "Halt";
                        }
                        RecvError::ClosedAndEmpty => {
                            println!("Channel is closed - Exiting now...");
                            break "Closed";
                        }
                    },
                }
            }
        },
    );

    tokio::time::sleep(Duration::from_millis(10)).await;

    for num in 0..20 {
        address.send(num).await.unwrap()
    }

    pool.halt_all();
    let exits: Vec<Result<&str, JoinError>> = pool.collect().await;

    for exit in exits {
        match exit {
            Ok(exit) => {
                assert_eq!(exit, "Halt");
                println!("Actor exited with message: {exit}")
            }
            Err(error) => match error {
                JoinError::Panic(_) => todo!(),
                JoinError::Abort => todo!(),
            },
        }
    }
}
```