-- Executed only on a NEW development volume. Never rerun against a production DB.
\getenv api_password API_DB_PASSWORD
\getenv worker_password WORKER_DB_PASSWORD
SELECT format('CREATE ROLE audeniq_api LOGIN PASSWORD %L', :'api_password') \gexec
SELECT format('CREATE ROLE audeniq_worker LOGIN PASSWORD %L', :'worker_password') \gexec
