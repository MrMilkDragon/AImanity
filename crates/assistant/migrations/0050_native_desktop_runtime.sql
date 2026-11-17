ALTER TABLE device_runtimes DROP CONSTRAINT device_runtimes_provider_check;
UPDATE device_runtimes SET provider='native-desktop' WHERE provider='docker-desktop';
ALTER TABLE device_runtimes ADD CONSTRAINT device_runtimes_provider_check
 CHECK(provider IN ('native-desktop','android-emulator'));
