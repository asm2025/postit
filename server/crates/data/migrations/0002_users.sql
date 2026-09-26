CREATE TABLE users (
    id UUID PRIMARY KEY,
    oidc_issuer TEXT NOT NULL,
    oidc_subject TEXT NOT NULL,
    email CITEXT,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    display_name TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'member')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'active', 'disabled', 'deleting')),
    approved_at TIMESTAMPTZ,
    approved_by UUID REFERENCES users (id) ON DELETE SET NULL,
    last_seen_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (oidc_issuer, oidc_subject)
);

CREATE INDEX users_status_idx ON users (status);
CREATE INDEX users_role_status_idx ON users (role, status);
