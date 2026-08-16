CREATE TABLE IF NOT EXISTS supermasters (
    ip INET NOT NULL,
    nameserver VARCHAR(255) NOT NULL,
    account VARCHAR(40) NOT NULL,
    PRIMARY KEY (ip, nameserver)
);
