# tiny-actor
[![Crates.io](https://img.shields.io/crates/v/tiny-actor)](https://crates.io/crates/tiny-actor)
[![Documentation](https://docs.rs/tiny-actor/badge.svg)](https://docs.rs/tiny-actor)

Tiny-actor is a tiny tokio-based actor framework for Rust. 

Because tiny-actor tries to stay as minimal as possible, it can be used both in libraries, as well as in applications. The core idea of tiny-actor is merging `Inbox`es with `tokio::task`s: It's impossible to create an `Inbox` without a `tokio::task`. This principle allows for building simple pools and supervision-trees with reliable shutdown behaviour.

I have been trying to figure out the most what the best way is to write an actor-system in Rust. My current attempt, a full actor framework ([zestors](https://crates.io/crates/zestors)) will be using tiny-actor in the future.

# Concepts

## Channel
A `Channel` is that which underlies the coupling of `Inbox`es, `Address`es and `Child`ren. Every channel contains the following structs: 
* One `Child` or `ChildPool`
* Zero or more `Address`es
* Zero or more `Inbox`es

```other
|¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|
|                            Channel                          |
|  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |              Actor                |  |   Child(Pool)  |  |
|  |  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |  |________________|  |
|  |  |         Process(es)         |  |                      |
|  |  |  |¯¯¯¯¯¯¯¯¯¯¯¯|  |¯¯¯¯¯¯¯|  |  |  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |  |  | tokio-task |  | Inbox |  |  |  |  Address(es)   |  |
|  |  |  |____________|  |_______|  |  |  |________________|  |
|  |  |_____________________________|  |                      |
|  |___________________________________|                      |
|_____________________________________________________________|
```

## Actor
The term `Actor` is used to describe (multiple) `Process`es sharing a single `Channel`. The `Actor` appears to be functioning as a single unit to other `Process`es sending messages to it through it's `Address`.

## Process
The term `Process` is used to describe the coupling of an `Inbox` with a `tokio::task`. 

## Inbox
An `Inbox` is a receiver-part of the `Channel`, and is primarily used to take messages out of the `Channel`. `Inbox`es can only be created by spawning new `Process`es and should stay coupled to the `tokio::task` they were spawned with. Therefore, an `Inbox` should only be dropped when the `tokio::task` is exiting.

## Address
An `Address` is the cloneable sender-part of a `Channel`, and is primarily used to send messages to the `Actor`. When all `Address`es are dropped, the `Channel` is closed automatically. `Address`es can be awaited, which will return when the `Actor` has exited.

## Child
A `Child` is a handle to an `Actor` with a single `Process`. A `Child` can be awaited to return the exit-value of the `tokio::task`. A `Child` is non-cloneable, and therefore unique to the `Channel`. When the `Child` is dropped, the `Actor` will be `halt`ed and `abort`ed. This can be prevented by detaching the `Child`. More processes can be spawned later, which transforms the `Child` into a `ChildPool`.

## ChildPool
A `ChildPool` is similar to a `Child`, except that the `Actor` can have more than one `Process`. A `ChildPool` can be streamed to get the exit-values of all spawned `tokio::task`s.

## Closing
When a `Channel` is closed, it is not longer possible to send new messages into it. It is still possible to take out any messages that are left. The processes of a closed `Channel` do not have to exit necessarily. Any senders are notified with a `SendError::Closed`, while receivers will receive `RecvError::ClosedAndEmpty` once the `Channel` has been emptied.

## Halting
A `Process` can be `halt`ed exactly once, by receiving a `RecvError::Halted`. Afterwards the `Process` should exit. An `Actor` can be partially halted, meaning that only some of the `Processes`es have been `halt`ed.

## Aborting
An `Actor` can be `abort`ed through tokio's [abort](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html#method.abort) method. This causes the `tokio::task`s to exit abruptly, and can leave bad state behind. Wherever possible, use `halt` instead of `abort`. By default `Process`es are automatically aborted when the `Child/ChildPool` is dropped. This can be prevented by detaching the `Child/ChildPool`.

## Exiting
Exit can refer to two seperate events which, with good practise, always occur at the same time:
* A `Process` can exit by dropping it's `Inbox`. Once all `Inbox`es of a `Channel` have been dropped, the `Actor` has exited. This type of exit can be retrieved/awaited from the `Channel` at any time.
* A `tokio::task` can exit, which means the process is no longer alive. This can only be queried only once, by awaiting the `Child` or `ChildPool` 

Therefore, it is recommended to drop an `Inbox` only when the `tokio::task` is also exiting. This way, an exit always refers to the same event.

## Abort-timer
If an `Actor` is attached, the `Child/ChildPool` has an `abort-timer`. Upon dropping the `Child/ChildPool` instantly a `Halt`-signal is sent to all inboxes. After the `abort-timer`, if the `tokio::task` still has not exited, the `Actor` is `abort`ed.

## Capacity
A `Channel` can either be bounded or unbounded. A bounded `Channel` can receive messages until it's capacity has been reached. After reaching the capacity, senders must wait until space is available. An unbounded `Channel` does not have this limit, but instead applies a backpressure-algorithm: The more messages in the `Channel`, the longer the sender must wait before it is allowed to send. 

## Default Config
* `Attached` with an abort-timer of `1 sec`. 
* `Unbounded` capacity with BackPressure timeout starting from `5 messages` at `25ns` with an `exponential` growth-factor of `1.3`.



# Examples

## Basic
```rust
use tiny_actor::*;
use std::time::Duration;

#[tokio::main]
async fn main() {
    let (child, address) = spawn(Config::default(), |mut inbox: Inbox<u32>| async move {
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

    tokio::time::sleep(Duration::from_millis(10));

    child.halt();

    match child.await {
        Ok(exit) => {
            assert_eq!(exit, "Halt");
            println!("Actor exited with message: {exit}")
        },
        Err(error) => match error {
            ExitError::Panic(_) => todo!(),
            ExitError::Abort => todo!(),
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
            link: Link::Attached(Duration::from_secs(1)),
            capacity: Capacity::Unbounded(BackPressure {
                start_at: 5,
                timeout: Duration::from_nanos(25),
                growth: Growth::Exponential(1.3),
            }),
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

    for num in 0..10 {
        address.send(num).await.unwrap()
    }

    pool.halt_all();
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
```