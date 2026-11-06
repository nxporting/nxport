#!/usr/bin/perl
# The licence texts of what the builder is built from, as one file for the release archive: each crate the build reaches
# through normal dependencies, its own licence files. Usage: notices.pl <output file> [target triple]
use strict;
use warnings;
use JSON::PP;

my ($out, $target) = @ARGV;
die "usage: notices.pl <output> [target]\n" unless $out;
my $filter = $target ? "--filter-platform $target" : "";
my $json = `cargo metadata --format-version 1 --locked $filter`;
die "cargo metadata failed\n" if $?;
my $meta = decode_json($json);

my %member = map { $_ => 1 } @{ $meta->{workspace_members} };
my %node = map { $_->{id} => $_ } @{ $meta->{resolve}{nodes} };
my @todo = grep { /nxport-builder/ } @{ $meta->{workspace_members} };
die "no nxport-builder in the workspace\n" unless @todo;
my %reached;
while (defined(my $id = shift @todo)) {
    next if $reached{$id}++;
    for my $dep (@{ $node{$id}{deps} }) {
        push @todo, $dep->{pkg} if grep { !defined $_->{kind} } @{ $dep->{dep_kinds} };
    }
}

my @crates = sort { $a->{name} cmp $b->{name} || $a->{version} cmp $b->{version} }
    grep { $reached{ $_->{id} } && !$member{ $_->{id} } } @{ $meta->{packages} };
open(my $o, '>:raw', $out) or die "$out: $!\n";
my $missing = 0;
for my $c (@crates) {
    my $license = $c->{license} // 'see the files below';
    print $o ('=' x 72) . "\n$c->{name} $c->{version}\nLicense: $license\n" . ('=' x 72) . "\n\n";
    (my $dir = $c->{manifest_path}) =~ s{[\\/]Cargo\.toml$}{};
    $dir =~ s{\\}{/}g;
    opendir(my $d, $dir) or die "$dir: $!\n";
    my @files = sort grep { /^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE)/i && -f "$dir/$_" } readdir $d;
    closedir $d;
    for my $f (@files) {
        open(my $i, '<:raw', "$dir/$f") or die "$dir/$f: $!\n";
        local $/;
        print {$o} scalar(<$i>), "\n\n";
    }
    if (!@files) {
        print $o "(the crate holds no licence file; its manifest says: $license)\n\n";
        print STDERR "no licence file: $c->{name} $c->{version} ($license)\n";
        $missing++;
    }
}
close $o;
printf "%d crates, %d without a licence file, %d bytes\n", scalar @crates, $missing, -s $out;
