-- distinguish an explicit user strategy from metric-derived strategy.
-- Nullable means automatic/derived; non-null is the user's persisted override.
ALTER TABLE vl_loop_delegations ADD COLUMN strategy_override TEXT;
