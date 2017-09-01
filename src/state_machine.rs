
use std::time::{Instant};
use std::collections::{HashSet, HashMap};

use ::shares::{DataShare};
use ::messages::{MESSAGE_ID_LEN,
    unite_message, correct_frag_message};

struct CurMessage {
    b: u8,
    data_shares: HashMap<u8, Vec<u8>>, // input -> data
}

struct FragStateMachine {
    used_message_ids: HashMap<[u8; MESSAGE_ID_LEN], Instant>,
    cur_messages: HashMap<[u8; MESSAGE_ID_LEN], CurMessage>,
}


/*
- messageId         [8 bytes]
- b                 [1 byte]
- shareIndex        [1 byte]
- shareData         [variable amount of bytes]
- errorCorrection   [8 bytes]
*/

impl FragStateMachine {
    /// Process a newly received Fragmentos message.
    /// Possibly return a reconstructed message.
    fn received_frag_message(&mut self, frag_message: &[u8], cur_time: Instant) -> Option<Vec<u8>> {

        // Use the error correcting code to try to correct the error (If damaged):
        match correct_frag_message(frag_message) {
            true => {},
            false => {return None;},
        };

        let message_id = frag_message[0 .. MESSAGE_ID_LEN];

        if self.contains_key(message_id) {
            self.used_message_ids.insert(message_id, cur_time);

        }


        let b = frag_message[MESSAGE_ID_LEN];
        let share_index = frag_message[MESSAGE_ID_LEN + 1];


    }

    /// Notice about the passing time.
    /// Possibly use this to clean up old entries.
    fn time_tick(&mut self, cur_time: Instant) {

    }
}
