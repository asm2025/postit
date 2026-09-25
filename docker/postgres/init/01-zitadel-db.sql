-- Runs once, on first container init only (postgres image convention). Zitadel manages
-- its own schema inside this database; postit's own migrations run against `postit`.
CREATE DATABASE zitadel;
