INSERT INTO life_capability_catalog
    (capability,allowed_tools,risk_class,requires_expected_version,
     default_max_calls,max_batch_size,obligations,catalog_version,status)
VALUES
('write_command:preview','["preview_life_write"]','medium',true,5,1,'[]',1,'active'),
('write_command:execute','["execute_confirmed_life_write"]','high',true,1,1,
 '["human_confirmation","step_up_authentication"]',1,'active');
