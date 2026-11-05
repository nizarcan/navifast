# frozen_string_literal: true

# native-packages supplies an owned copy, already signed when Apple credentials
# are configured. Keep it unchanged and package only this input.
require "fileutils"
require "find"
require "tmpdir"

payload, output = ARGV
abort "usage: dmg.rb PAYLOAD OUTPUT.dmg" unless payload && output
abort "output already exists: #{output}" if File.exist?(output)
Dir.mktmpdir("navifast-dmg-") do |directory|
  FileUtils.cp_r(File.join(payload, "."), directory, preserve: true)
  app = File.join(directory, "Navifast.app")
  abort "expected only the signed Navifast.app input" unless File.directory?(app) &&
    !File.exist?(File.join(directory, "Fastpotify.app"))
  File.symlink("/Applications", File.join(directory, "Applications"))
  # Adapted from crmne/spotifast commit 3563c57869264f4514de3c76e4d87ef67f572ade.
  # hdiutil's estimate can be too small, so reserve room for the filesystem.
  megabytes = Find.find(directory).sum { |path| File.lstat(path).size } / (1024 * 1024)
  size = "#{(megabytes * 3 / 2) + 64}m"
  # HFS+ keeps the distributable image compatible with every supported Mac
  # and avoids APFS creation leaving a working image attached after success.
  abort "DMG creation failed" unless system("hdiutil", "create", "-volname", "Navifast",
    "-srcfolder", directory, "-fs", "HFS+", "-format", "UDZO", "-size", size, output)
  abort "DMG verification failed" unless system("hdiutil", "verify", output)
end
