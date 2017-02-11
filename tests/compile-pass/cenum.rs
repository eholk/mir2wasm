#![feature(lang_items, no_core)]
#![allow(dead_code)]
#![no_core]
#![allow(unused_variables)]

#[lang="sized"]
trait Sized {}

#[lang="copy"]
trait Copy {}

enum Tag {
    A = -1,
    B = 0,
    C = 1,
}

fn main() {
    let a = Tag::A;
    let a = match a {
        Tag::A => 2,
        Tag::B => 3,
        Tag::C => 4,
    };
}
