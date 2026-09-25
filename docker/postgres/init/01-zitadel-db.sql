-- Runs once, on first container init only (postgres image convention). Zitadel manages
-- its own schema inside this database; postly's own migrations run against `postly`.
CREATE DATABASE zitadel;
