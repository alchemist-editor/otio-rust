//! A program that depends on the released Zig package the way a user's
//! would, from `otio/` beside it, and runs. The release workflow unpacks
//! `otio-zig-<version>.tar.gz` there on every platform it ships for, so a
//! target whose library is missing from the package, or does not link,
//! fails before the release is published.

const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const otio = b.dependency("otio", .{ .target = target, .optimize = optimize });

    const exe = b.addExecutable(.{
        .name = "zig-package-smoke",
        .root_module = b.createModule(.{
            .root_source_file = b.path("main.zig"),
            .target = target,
            .optimize = optimize,
            .imports = &.{.{ .name = "otio", .module = otio.module("otio") }},
        }),
    });
    const run = b.addRunArtifact(exe);
    run.expectStdErrEqual("clips: 2\n");
    b.step("smoke", "Build and run against the unpacked package").dependOn(&run.step);
}
