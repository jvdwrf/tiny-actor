# tiny-actor
[![Crates.io](https://img.shields.io/crates/v/tiny-actor)](https://crates.io/crates/tiny-actor)
[![Documentation](https://docs.rs/tiny-actor/badge.svg)](https://docs.rs/tiny-actor)

Tiny-actor is a minimal and unopinionated Actor framework for Rust.

The main principle of tiny-Actor is merging `Inbox`es with `tasks`: It's impossible to create an `Inbox` without a `task`. Following this principle allows us to buildi simple pools and supervision-trees with reliable shutdown behaviour.

This library will not be trying out any API's similar to Actix's, Instead I'm planning to build another actor-library that will use tiny-actor under the hood.

# Concepts

## Actor
An `Actor` is that which couples `Inboxes`, `Addresses` and `Children` together. Every unique `Actor` contains the following items: 
* A single `Child(Pool)`
* One or more `Addresses`
* One or more `processes`, which constist of:
  * A single `task`
  * A single `Inbox`

The `Actor` appears to be functioning as a single unit to any processing communicating with it through it's `Address`.

```other
|¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|
|                      Actor       |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |   Child(Pool)  |  |
|  |         process(es)        |  |________________|  |
|  |  |¯¯¯¯¯¯¯¯¯|  |¯¯¯¯¯¯¯¯¯|  |                      |
|  |  |  task   |  |  Inbox  |  |  |¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯¯|  |
|  |  |_________|  |_________|  |  |  Address(es)   |  |
|  |____________________________|  |________________|  |
|______________________________________________________|
```

## Process
The term `process` is used to describe the coupling of an `Inbox` with a `task`.

## Inbox
An `Inbox` is a receiver to the `Actor`, and is primarily used to take messages out of the `Actor`. `Inboxes` can be created by spawning new `processes` and should stay coupled to the `task` they were spawned with: An `Inbox` should only be dropped when the `task` is exiting.

## Address
An `Address` is the clone-able sender of an `Actor`, and is primarily used to send messages to the `Actor`. When all `Addresses` are dropped, the `Actor` is closed automatically. `Addresses` can be awaited, which will wait until the `Actor` has exits.

## Child(Pool)
A `Child` is a handle to an `Actor` with one `process`. The `Child` can be awaited to return the exit-value of the `task`. The `Child` is not clone-able, and therefore unique to the `Actor`. When the `Child` is dropped, the `Actor` will be `halted` and subsequently `aborted`. This can be prevented by detaching the `Child`. 

A `ChildPool` is similar to a `Child`, except that the `Actor` can have multiple `processes`. The `ChildPool` can be streamed to get the exit-values of all spawned `tasks`. More `processes` can be spawned after the `Actor` has been spawned, and it's also possible to `halt` a portion of the `processes` of the `Actor`.

## Closing
When an `Actor` is `closed`, it is not longer possible to send new messages into it. It is still possible to take out any messages that are left. The `processes` of a closed `Actor` do not have to exit necessarily, but can continue running. Any senders are notified with a `SendError::Closed`, while receivers will receive `RecvError::ClosedAndEmpty` once the `Actor` has been emptied.

## Halting
A `process` can be `halted` exactly once, by receiving a `RecvError::Halted`. Afterwards the `process` should exit. An `Actor` can be partially halted, meaning that only some of the `processeses` have been `halted`.

## Aborting
An `Actor` can be `aborted` through tokio's [abort](https://docs.rs/tokio/latest/tokio/task/struct.JoinHandle.html#method.abort) method. This causes the `tasks` to exit abruptly, and can leave bad state behind. Wherever possible, use `halt` instead of `abort`. By default `processes` are automatically `aborted` when the `Child(Pool)` is dropped. This can be prevented by detaching the `Child(Pool)`.

## Exiting
An `exit` can refer to two seperate events which, with good practise, always occur at the same time:
* A `process` can exit by dropping it's `Inbox`. Once all `Inboxes` of an `Actor` have been dropped, the `Actor` has `exited`. This type of exit can be retrieved from the `Actor` at any time using `has_exited`.
* A `task` can exit, which means the `task` is no longer alive. This can only be queried only once, by awaiting the `Child(Pool)` or by calling `is_finished`. 

Therefore, it is recommended to drop an `Inbox` only when the `task` is also exiting, this way an exit always refers to the same event.

## Link
An `Actor` can either be `attached` or `detached`, which indicates what should happen when the `Child(Pool)` is dropped. If it is `attached`, then it will automatically `halt` all `processes`, and after the `abort-timer`, all processes will be `aborted`. If it is `detached`, then nothing happens when the `Child(Pool)` is dropped.

## Capacity
An `Actor` can either be bounded or unbounded. A bounded `Actor` can receive messages until it's capacity has been reached. After reaching the capacity, senders must wait until space is available. An unbounded `Actor` does not have this limit, but instead applies a backpressure-algorithm: The more messages in the `Actor`, the longer the sender must wait before it is allowed to send. 

## Default Config
* `Attached` with an abort-timer of `1 sec`. 
* `Unbounded` capacity with BackPressure timeout starting from `5 messages` at `25ns` with an `exponential` growth-factor of `1.3`.

# Examples

## Basic
```ignore
use tiny_actor::*;
use std::time::Duration;

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
    Two(Echo)
}

#[tokio::main]
async fn main() {
    // Spawn an process with a default config, and an Inbox<MyProtocol>
    let (child, address) = spawn(Config::default(), |mut inbox: Inbox<MyProtocol>| async move {
        loop {
            match inbox.recv().await {
                Ok(msg) => println!("Received message: {msg}"),
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
    });

    address.send(Echo("hi".to_string())).await.unwrap();
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
```ignore
use tiny_actor::*;
use std::time::Duration;
use futures::stream::StreamExt;

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
        |i, mut inbox: Inbox<u32>| async move {
            loop {
                match inbox.recv().await {
                    Ok(msg) => println!("Received message on Actor {i}: {msg}"),
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
        address.send(num).await.unwrap()
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
```