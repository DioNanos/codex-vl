-- Additive per-loop suspension state. 0933 remains untouched.
ALTER TABLE vl_loop_delegations ADD COLUMN cooldown_until_ms INTEGER;
ALTER TABLE vl_loop_delegations ADD COLUMN suspend_reason TEXT;
