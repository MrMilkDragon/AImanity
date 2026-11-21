ALTER TABLE harness_events ADD COLUMN input_received_at timestamptz;
CREATE INDEX harness_action_receipts ON harness_events(message_id,input_received_at,id)
    WHERE input_received_at IS NOT NULL;
