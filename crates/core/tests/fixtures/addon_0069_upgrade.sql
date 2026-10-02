DO $$
DECLARE org uuid:=gen_random_uuid(); party uuid:=gen_random_uuid(); usr uuid:=gen_random_uuid();
 artist uuid:=gen_random_uuid(); release uuid:=gen_random_uuid(); free_order uuid:=gen_random_uuid(); cancelled_order uuid:=gen_random_uuid();
BEGIN
 INSERT INTO identity.orgs(id,name,kind) VALUES(org,'Upgrade fixture','PERSONAL');
 INSERT INTO identity.parties(id,org_id,kind,display_name) VALUES(party,org,'PERSON','Fixture');
 INSERT INTO identity.users(id,email,password_hash,party_id) VALUES(usr,'upgrade-fixture@example.test','fixture',party);
 INSERT INTO identity.memberships(org_id,user_id,role) VALUES(org,usr,'OWNER');
 INSERT INTO identity.staff_members(user_id,role,granted_by) VALUES(usr,'ADMIN','upgrade-fixture');
 INSERT INTO identity.resources(org_id,id,kind) VALUES(org,artist,'artist'),(org,release,'release'),(org,free_order,'addon_order'),(org,cancelled_order,'addon_order');
 INSERT INTO catalog.artists(id,org_id,name) VALUES(artist,org,'Upgrade fixture');
 INSERT INTO catalog.releases(id,org_id,title,release_type) VALUES(release,org,'Upgrade fixture','SINGLE');
 UPDATE catalog.addon_service_catalog SET active=false WHERE code='PROFILE_PLUS';
 INSERT INTO catalog.addon_service_catalog(id,code,category,display_name,description,price_krw,billing_unit,active,requires_payment,validity_days,validity_months,version)
 VALUES(gen_random_uuid(),'PROFILE_PLUS','ARTIST_PROFILE','Profile Plus','Sponsored',0,'artist',true,false,365,12,2);
 INSERT INTO catalog.addon_orders(id,org_id,requester_user_id,service_code,catalog_version,price_snapshot_krw,amount,currency,payment_status,status,target_type,target_id,artist_id,validity_days_snapshot,validity_months_snapshot,submitted_at,completed_at)
 VALUES(free_order,org,usr,'PROFILE_PLUS',2,0,0,'KRW','NOT_REQUIRED','COMPLETED','artist',artist,artist,365,12,now()-interval '2 months',now()-interval '1 month');
 INSERT INTO catalog.addon_orders(id,org_id,requester_user_id,service_code,catalog_version,price_snapshot_krw,amount,currency,payment_status,status,target_type,target_id,release_id,priority,paid_at,payment_reference,cancelled_at,refund_status)
 VALUES(cancelled_order,org,usr,'PRIORITY_DELIVERY',1,10000,10000,'KRW','PAID','CANCELLED','release',release,release,10,now(),'upgrade-payment',now(),'REQUESTED');
 INSERT INTO operations.jobs(id,queue,kind,payload,idempotency_key,priority,addon_priority_previous,status,lock_token,lease_until)
 VALUES(gen_random_uuid(),'rights','test.upgrade',jsonb_build_object('release_id',release),'upgrade-running',10,0,'RUNNING',gen_random_uuid(),now()+interval '1 hour');
 INSERT INTO operations.jobs(id,queue,kind,payload,idempotency_key,priority,addon_priority_previous)
 VALUES(gen_random_uuid(),'qc','test.upgrade',jsonb_build_object('release_id',release),'upgrade-urgent',20,0);
END $$;
