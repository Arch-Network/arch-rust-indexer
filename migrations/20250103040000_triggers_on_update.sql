-- Ensure triggers run on UPDATE as well as INSERT so UPSERTs fire the logic
-- Account participation trigger
DROP TRIGGER IF EXISTS account_participation_trigger ON transactions;
CREATE TRIGGER account_participation_trigger
AFTER INSERT OR UPDATE ON transactions
FOR EACH ROW EXECUTE FUNCTION populate_account_participation();

-- Transaction programs trigger
DROP TRIGGER IF EXISTS transaction_programs_trigger ON transactions;
CREATE TRIGGER transaction_programs_trigger
AFTER INSERT OR UPDATE ON transactions
FOR EACH ROW EXECUTE FUNCTION update_transaction_programs();
