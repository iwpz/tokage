-- Execution material is scoped by session + database plan (and thus plan version) + task code.
-- Preserve legacy rows. Only an exact attempt -> database task -> plan chain proves ownership.
ALTER TABLE task_reports ADD COLUMN session_id TEXT REFERENCES main_sessions(id) ON DELETE CASCADE;
ALTER TABLE task_reports ADD COLUMN plan_record_id INTEGER REFERENCES plans(id) ON DELETE CASCADE;
ALTER TABLE task_reports ADD COLUMN association_status TEXT NOT NULL DEFAULT 'BOUND'
    CHECK (association_status IN ('BOUND','LEGACY_UNASSOCIATED'));
UPDATE task_reports SET association_status = 'LEGACY_UNASSOCIATED';
CREATE INDEX idx_task_reports_identity ON task_reports(session_id, plan_record_id, task_id, id);
ALTER TABLE briefs ADD COLUMN session_id TEXT REFERENCES main_sessions(id) ON DELETE CASCADE;
ALTER TABLE briefs ADD COLUMN plan_record_id INTEGER REFERENCES plans(id) ON DELETE CASCADE;
ALTER TABLE briefs ADD COLUMN association_status TEXT NOT NULL DEFAULT 'BOUND'
    CHECK (association_status IN ('BOUND','LEGACY_UNASSOCIATED'));
UPDATE briefs SET association_status = 'LEGACY_UNASSOCIATED';
CREATE INDEX idx_briefs_identity ON briefs(session_id, plan_record_id, task_id, id);
ALTER TABLE gate_runs ADD COLUMN session_id TEXT REFERENCES main_sessions(id) ON DELETE CASCADE;
ALTER TABLE gate_runs ADD COLUMN plan_record_id INTEGER REFERENCES plans(id) ON DELETE CASCADE;
ALTER TABLE gate_runs ADD COLUMN association_status TEXT NOT NULL DEFAULT 'BOUND'
    CHECK (association_status IN ('BOUND','LEGACY_UNASSOCIATED'));
UPDATE gate_runs SET association_status = 'LEGACY_UNASSOCIATED';
CREATE INDEX idx_gate_runs_identity ON gate_runs(session_id, plan_record_id, task_id, id);
UPDATE task_reports AS x SET
 session_id = (SELECT p.main_session_id FROM attempts a JOIN tasks t ON t.id=a.task_id JOIN plans p ON p.id=t.plan_id
               WHERE a.id=x.attempt_id AND t.code=x.task_id AND a.main_session_id=p.main_session_id),
 plan_record_id = (SELECT t.plan_id FROM attempts a JOIN tasks t ON t.id=a.task_id JOIN plans p ON p.id=t.plan_id
               WHERE a.id=x.attempt_id AND t.code=x.task_id AND a.main_session_id=p.main_session_id);
UPDATE task_reports SET association_status='BOUND' WHERE session_id IS NOT NULL AND plan_record_id IS NOT NULL;
UPDATE gate_runs AS x SET
 session_id = (SELECT p.main_session_id FROM attempts a JOIN tasks t ON t.id=a.task_id JOIN plans p ON p.id=t.plan_id
               WHERE a.id=x.attempt_id AND t.code=x.task_id AND a.main_session_id=p.main_session_id AND CAST(t.plan_id AS TEXT) = x.plan_id),
 plan_record_id = (SELECT t.plan_id FROM attempts a JOIN tasks t ON t.id=a.task_id JOIN plans p ON p.id=t.plan_id
               WHERE a.id=x.attempt_id AND t.code=x.task_id AND a.main_session_id=p.main_session_id AND CAST(t.plan_id AS TEXT) = x.plan_id);
UPDATE gate_runs SET association_status='BOUND' WHERE session_id IS NOT NULL AND plan_record_id IS NOT NULL;
CREATE TRIGGER task_reports_identity_insert BEFORE INSERT ON task_reports
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p JOIN tasks t ON t.plan_id=p.id WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND t.code=NEW.task_id) AND EXISTS (SELECT 1 FROM attempts a JOIN tasks t ON t.id=a.task_id WHERE a.id=NEW.attempt_id AND a.main_session_id=NEW.session_id AND t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id))
 THEN RAISE(ABORT, 'invalid task_reports execution identity') END;
END;
CREATE TRIGGER task_reports_identity_update BEFORE UPDATE ON task_reports WHEN NEW.association_status='BOUND'
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p JOIN tasks t ON t.plan_id=p.id WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND t.code=NEW.task_id) AND EXISTS (SELECT 1 FROM attempts a JOIN tasks t ON t.id=a.task_id WHERE a.id=NEW.attempt_id AND a.main_session_id=NEW.session_id AND t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id))
 THEN RAISE(ABORT, 'invalid task_reports execution identity') END;
END;
CREATE TRIGGER briefs_identity_insert BEFORE INSERT ON briefs
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p JOIN tasks t ON t.plan_id=p.id WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND t.code=NEW.task_id))
 THEN RAISE(ABORT, 'invalid briefs execution identity') END;
END;
CREATE TRIGGER briefs_identity_update BEFORE UPDATE ON briefs WHEN NEW.association_status='BOUND'
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p JOIN tasks t ON t.plan_id=p.id WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND t.code=NEW.task_id))
 THEN RAISE(ABORT, 'invalid briefs execution identity') END;
END;
CREATE TRIGGER gate_runs_identity_insert BEFORE INSERT ON gate_runs
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND CAST(p.id AS TEXT)=NEW.plan_id) AND (NEW.task_id IS NULL OR EXISTS (SELECT 1 FROM tasks t WHERE t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id)) AND (NEW.attempt_id IS NULL OR EXISTS (SELECT 1 FROM attempts a JOIN tasks t ON t.id=a.task_id WHERE a.id=NEW.attempt_id AND a.main_session_id=NEW.session_id AND t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id)))
 THEN RAISE(ABORT, 'invalid gate_runs execution identity') END;
END;
CREATE TRIGGER gate_runs_identity_update BEFORE UPDATE ON gate_runs WHEN NEW.association_status='BOUND'
BEGIN
 SELECT CASE WHEN NEW.association_status != 'BOUND' OR NOT (EXISTS (SELECT 1 FROM plans p WHERE p.id=NEW.plan_record_id AND p.main_session_id=NEW.session_id AND CAST(p.id AS TEXT)=NEW.plan_id) AND (NEW.task_id IS NULL OR EXISTS (SELECT 1 FROM tasks t WHERE t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id)) AND (NEW.attempt_id IS NULL OR EXISTS (SELECT 1 FROM attempts a JOIN tasks t ON t.id=a.task_id WHERE a.id=NEW.attempt_id AND a.main_session_id=NEW.session_id AND t.plan_id=NEW.plan_record_id AND t.code=NEW.task_id)))
 THEN RAISE(ABORT, 'invalid gate_runs execution identity') END;
END;

-- Direct inserts cannot attach an attempt to a task in another session.
CREATE TRIGGER attempts_task_identity_insert BEFORE INSERT ON attempts
WHEN NEW.task_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM attempts WHERE id=NEW.id)
BEGIN
 SELECT CASE WHEN NOT EXISTS (SELECT 1 FROM tasks t JOIN plans p ON p.id=t.plan_id
   WHERE t.id=NEW.task_id AND p.main_session_id=NEW.main_session_id)
 THEN RAISE(ABORT, 'invalid attempt task identity') END;
END;
CREATE TRIGGER attempts_task_identity_update BEFORE UPDATE OF task_id, main_session_id ON attempts
WHEN NEW.task_id IS NOT OLD.task_id OR NEW.main_session_id IS NOT OLD.main_session_id
BEGIN
 SELECT RAISE(ABORT, 'attempt execution identity is immutable');
END;
