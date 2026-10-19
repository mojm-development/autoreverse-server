-- Tokens are stored as their SHA-256 (lower-case hex) from now on, see src/auth/tokens.rs.
-- Existing sessions stay valid: their plain value is hashed in place.
UPDATE auth_tokens SET value = encode(sha256(convert_to(value, 'UTF8')), 'hex');
