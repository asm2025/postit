CREATE TABLE user_preferences (
    user_id UUID PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    email_notifications BOOLEAN NOT NULL DEFAULT TRUE,
    timezone TEXT NOT NULL DEFAULT 'UTC',
    store_ai_prompts BOOLEAN NOT NULL DEFAULT FALSE,
    last_approval_email_at TIMESTAMPTZ
);
