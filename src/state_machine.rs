use std::time::{Instant};
use std::collections::{HashMap};

use ::shares::{DataShare};
use ::messages::{MESSAGE_ID_LEN, ECC_LEN,
    unite_message, correct_frag_message};

const MESSAGE_ID_TIMEOUT: u64 = 30;

struct CurMessage {
    instant_added: Instant,
    b: u8,
    share_length: usize,
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
    fn new() -> Self {
        FragStateMachine {
            used_message_ids: HashMap::new(),
            cur_messages: HashMap::new(),
        }
    }

    /// Process a newly received Fragmentos message.
    /// Possibly return a reconstructed message.
    fn received_frag_message(&mut self, frag_message: &[u8], 
                             cur_instant: Instant) -> Option<Vec<u8>> {

        // Use the error correcting code to try to correct the error if possible.
        let corrected = match correct_frag_message(frag_message) {
            Some(corrected) => corrected,
            None => {return None;},
        };

        let message_id = array_ref![corrected, 0, MESSAGE_ID_LEN];

        if self.used_message_ids.contains_key(message_id) {
            // Refresh message_id entry inside used_message_ids:
            self.used_message_ids.insert(message_id.clone(), cur_instant);
            return None;
        }

        let share_length = 
            frag_message.len() - MESSAGE_ID_LEN - 1 - 1 - ECC_LEN;

        let b = frag_message[MESSAGE_ID_LEN];
        let share_index = frag_message[MESSAGE_ID_LEN + 1];
        let share_data = &frag_message[MESSAGE_ID_LEN + 1 + 1 ..  frag_message.len() - ECC_LEN];

        match self.cur_messages.contains_key(message_id) {
            true =>  {
                let cur_m = self.cur_messages.get(message_id).unwrap();
                // If there is already cur_m with the given message_id, make sure that it
                // matches the received fragment metadata:
                if cur_m.b != b {
                    return None;
                }
                if cur_m.share_length != share_length {
                    return None;
                }
            },
            false => {
                self.cur_messages.insert(message_id.clone(), CurMessage {
                    instant_added: cur_instant,
                    b,
                    share_length,
                    data_shares: HashMap::new(),
                });
            }
        };

        { 
            let cur_m = self.cur_messages.get_mut(message_id).unwrap();

            // If we already have this share, we discard the message:
            if cur_m.data_shares.contains_key(&share_index) {
                return None;
            }

            // Insert the new share we have received:
            cur_m.data_shares.insert(share_index, share_data.to_vec());

            if cur_m.data_shares.len() < b as usize {
                return None;
            }

            // We got b shares. This should be enough to try and reconstruct the full message.
            self.used_message_ids.insert(message_id.clone(), cur_instant);
        }

        let cur_m = self.cur_messages.remove(message_id).unwrap();

        let mut data_shares = cur_m.data_shares.into_iter()
                              .map(|(input, data)| DataShare {
                                  input, 
                                  data,
                              }).collect::<Vec<DataShare>>();

        // Avoid non determinism by sorting:
        data_shares.sort();
        Some(match unite_message(message_id, &data_shares) {
            Ok(m) => m,
            Err(_) => {return None;}
        })

    }

    /// A notice about the passing time.
    /// Possibly use this to clean up old entries.
    fn time_tick(&mut self, cur_instant: Instant) {

        // Cleanup old entries from cur_messages. 
        // For any such cleaned up message, move its message_id to used_message_ids.
        {
            let used_message_ids = &mut self.used_message_ids;
            self.cur_messages.retain(|message_id, cur_message| {
                if cur_instant.duration_since(cur_message.instant_added).as_secs() 
                        <= MESSAGE_ID_TIMEOUT {
                    true
                } else {
                    used_message_ids.insert(message_id.clone(), cur_instant);
                    false
                }
            });
        }

        // Cleanup old entries from used_message_ids:
        self.used_message_ids.retain(|_, &mut instant| {
            cur_instant.duration_since(instant).as_secs() <= MESSAGE_ID_TIMEOUT
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use ::messages::split_message;
    use std::time::Duration;

    #[test]
    fn test_time_tick_basic() {
        let mut fsm = FragStateMachine::new();
        let inst = Instant::now();
        let inst2 = inst + Duration::new(5,0);
        let inst3 = inst2 + Duration::new(5,0);

        fsm.time_tick(inst);
        fsm.time_tick(inst2);
        fsm.time_tick(inst3);
    }

    #[test]
    fn test_received_frag_message_basic() {
        let mut fsm = FragStateMachine::new();
        let cur_inst = Instant::now();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let b = (frags.len() + 1) / 2;
        for i in 0 .. b-1 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }
        let united = fsm.received_frag_message(&frags[frags.len() - 1], cur_inst).unwrap();
        assert_eq!(united, orig_message);
    }

    #[test]
    fn test_received_frag_same() {
        let mut fsm = FragStateMachine::new();
        let cur_inst = Instant::now();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let b = (frags.len() + 1) / 2;
        for i in 0 .. b-2 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }
        // Receive the same frag many times:
        for _ in 0 .. 100 {
            assert_eq!(fsm.received_frag_message(&frags[b-2], cur_inst), None);
        }

        let united = fsm.received_frag_message(&frags[frags.len() - 1], cur_inst).unwrap();
        assert_eq!(united, orig_message);
    }

    #[test]
    fn test_received_frag_late() {
        let mut fsm = FragStateMachine::new();
        let mut cur_inst = Instant::now();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let b = (frags.len() + 1) / 2;
        for i in 0 .. b-1 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }

        // A lot of time has passed...
        cur_inst += Duration::new(MESSAGE_ID_TIMEOUT + 1,0);
        fsm.time_tick(cur_inst);

        // Last frag is too late:
        assert_eq!(fsm.received_frag_message(&frags[frags.len() - 1], cur_inst), None);

        // Time moved a bit
        cur_inst += Duration::new(1,0);

        // We can't process the message again, because its id is inside the used_message_ids.
        for i in 0 .. b {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }

        // If we wait a bit, the message will be removed from used_message_ids.
        cur_inst += Duration::new(MESSAGE_ID_TIMEOUT + 1,0);
        fsm.time_tick(cur_inst);

        // Now we should be able to get the same message again:
        for i in 0 .. b - 1 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }
        let united = fsm.received_frag_message(&frags[frags.len() - 1], cur_inst).unwrap();
        assert_eq!(united, orig_message);

    }

    #[test]
    fn test_received_frag_rest_frags_ignored() {
        let mut fsm = FragStateMachine::new();
        let mut cur_inst = Instant::now();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let b = (frags.len() + 1) / 2;
        for i in 0 .. b-1 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }

        // frag number b:
        let united = fsm.received_frag_message(&frags[frags.len() - 1], cur_inst).unwrap();
        assert_eq!(united, orig_message);

        // A litle time has passed:
        cur_inst += Duration::new(1,0);
        fsm.time_tick(cur_inst);

        // We now get all the other frags. All of them should be ignored:
        for i in b .. frags.len() {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
        }
    }

    #[test]
    fn test_received_frag_cur_messages_timeout() {
        let mut fsm = FragStateMachine::new();
        let mut cur_inst = Instant::now();

        let orig_message = b"This is some message to be split";
        let frags = split_message(orig_message, 
                                  b"nonce123", 22).unwrap();

        let b = (frags.len() + 1) / 2;
        for i in 0 .. b - 1 {
            assert_eq!(fsm.received_frag_message(&frags[i], cur_inst), None);
            cur_inst += Duration::new(MESSAGE_ID_TIMEOUT - 1,0);
            fsm.time_tick(cur_inst);
        }

        // Frag b is ignored, because after about the second frag sent the cur_message entry was
        // removed, and message id was moved to used_message_ids.
        assert_eq!(fsm.received_frag_message(&frags[frags.len() - 1], cur_inst), None);;
    }
}