-- Per-user UI preferences (editor theme, keymap, font and layout), edited by
-- the user themselves. Unknown or missing fields fall back to defaults.
ALTER TABLE users ADD COLUMN preferences jsonb NOT NULL DEFAULT '{}';
