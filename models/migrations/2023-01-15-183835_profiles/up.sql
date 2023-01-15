CREATE TABLE user_profiles(
uid integer NOT NULL PRIMARY KEY,
email TEXT,
nostr_notifications boolean Default false,
email_notifications boolean Default false,
img_url TEXT,
twitter_handle TEXT,
is_twitter_verified boolean Default false,
is_email_verified boolean Default false,
CONSTRAINT fk_id
FOREIGN KEY (uid)
REFERENCES users(uid)
);