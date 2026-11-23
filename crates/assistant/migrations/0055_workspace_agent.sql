ALTER TABLE registry_meta ADD COLUMN active_agent_id uuid REFERENCES agents(id);

UPDATE registry_meta m SET active_agent_id=d.default_agent_id
FROM devices d WHERE d.owner_id=m.owner_id AND d.id='phone-local-01';

ALTER TABLE runs ALTER COLUMN device_id DROP DEFAULT, ALTER COLUMN device_id DROP NOT NULL;
