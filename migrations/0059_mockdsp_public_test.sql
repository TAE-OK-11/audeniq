-- Expose the existing local MockDSP adapter as an explicitly selected test
-- destination. The adapter remains MOCK and never opens an external connection.
INSERT INTO distribution.dsp_registry(code, dsp_id, slug, name, region, delivery_format)
VALUES ('D-36','5b8019d3-ca5a-5a54-834c-1f46164f9873','mockdsp','MockDSP','GLOBAL','DDEX')
ON CONFLICT (code) DO NOTHING;

UPDATE execution.adapter_profiles
   SET dsp_id = '5b8019d3-ca5a-5a54-834c-1f46164f9873',
       display_name = 'MockDSP',
       delivery_enabled = true,
       capabilities = jsonb_set(capabilities, '{send_or_publish}', 'true'::jsonb),
       updated_at = now()
 WHERE partner_id = 'mockdsp' AND activation_kind = 'MOCK';
