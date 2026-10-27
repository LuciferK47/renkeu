-- Autosave bursts are folded into one history row (D11). The row remembers
-- the text the burst started from so the merged diff stays correct.
ALTER TABLE history ADD COLUMN base_text TEXT;
