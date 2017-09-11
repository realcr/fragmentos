#[macro_use]
extern crate arrayref;

// #[macro_use]
// extern crate tokio_core;

mod shares;
mod messages;
mod state_machine;
mod protocol;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
    }
}
