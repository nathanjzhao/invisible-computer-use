import datetime as dt
import unittest
from refresh import expiry, renew, renew_rpm


class Freshness(unittest.TestCase):
    def test_expiry_boundary(self):
        now = dt.datetime(2026, 10, 28, tzinfo=dt.timezone.utc)
        release = b'Date: Mon, 12 Oct 2026 00:00:00 +0000\nValid-Until: Wed, 11 Nov 2026 00:00:00 +0000\nSHA256:\n abc 9 main/binary-amd64/Packages\n'
        expiry(release, now)
        with self.assertRaises(ValueError): expiry(release, now + dt.timedelta(seconds=1))
        with self.assertRaises(ValueError): expiry(release.replace(b'Valid-Until:', b'Other:'), now)
        with self.assertRaises(ValueError): expiry(release + b'Valid-Until: Wed, 11 Nov 2026 00:00:00 +0000\n', now)
        new = renew(release, now)
        self.assertEqual(new.split(b'SHA256:')[1], release.split(b'SHA256:')[1])
        self.assertEqual(expiry(new, now), (now + dt.timedelta(days=30)).isoformat())

    def test_rpm_refresh_retains_native_librepo_document_shape(self):
        now = dt.datetime(2026, 10, 15, tzinfo=dt.timezone.utc)
        body = b'''<?xml version="1.0" encoding="UTF-8"?>
<repomd xmlns="http://linux.duke.edu/metadata/repo" xmlns:rpm="http://linux.duke.edu/metadata/rpm">
  <revision>1792000000</revision>
  <data type="primary"><checksum type="sha256">abc</checksum><location href="repodata/primary.xml.gz"/></data>
</repomd>
'''
        result = renew_rpm(body, now)
        self.assertEqual(result, body.replace(b'1792000000', str(int(now.timestamp())).encode(), 1))
        self.assertIn(b'<repomd xmlns=', result)
        self.assertNotIn(b'ns0:', result)

    def test_rpm_refresh_rejects_missing_or_ambiguous_revision(self):
        now = dt.datetime(2026, 10, 15, tzinfo=dt.timezone.utc)
        body = b'<repomd xmlns="http://linux.duke.edu/metadata/repo"><revision>1</revision></repomd>'
        for invalid in [body.replace(b'<revision>1</revision>', b''),
                        body.replace(b'<revision>1</revision>', b'<revision>1</revision><revision>2</revision>'),
                        body.replace(b'</repomd>', b'<!-- <revision>1</revision> --></repomd>')]:
            with self.assertRaises(ValueError): renew_rpm(invalid, now)


if __name__ == '__main__': unittest.main()
