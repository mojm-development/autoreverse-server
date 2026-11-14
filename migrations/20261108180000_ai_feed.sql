-- The suggestions last generated for a user, so that opening the page costs nothing.
CREATE TABLE ai_feed (
	user_id integer PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
	payload jsonb NOT NULL,
	generated_at timestamptz NOT NULL DEFAULT now()
);
