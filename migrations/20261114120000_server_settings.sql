-- What admins set under Einstellungen → Server (intervals of the background jobs, switches of
-- optional features, the language model): one JSON document in a table of one row. These were
-- environment variables before; the first start with this table takes them over once.
CREATE TABLE server_settings (
	id boolean PRIMARY KEY DEFAULT true CHECK (id),
	value jsonb NOT NULL,
	updated_at timestamptz NOT NULL DEFAULT now()
);
