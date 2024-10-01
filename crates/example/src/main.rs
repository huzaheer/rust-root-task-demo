//
// Copyright 2023, Colias Group, LLC
//
// SPDX-License-Identifier: BSD-2-Clause
//

#![no_std]
#![no_main]

use sel4_root_task::{root_task, Never};

#[root_task]
fn main(bootinfo: &sel4::BootInfoPtr) -> sel4::Result<Never> {
    sel4::debug_println!("Hello, World!");


    // Create Notification Object
    let blueprint = sel4::ObjectBlueprint::Notification;

    /* Untyped memory is a block of contiguous physical memory with a specific size. 
    Untyped capabilities are capabilities to untyped memory. 
    Untyped capabilities can be retyped into kernel objects together 
    with capabilities to them, or into further, usually smaller, untyped capabilities. */


    // Iterate through untyped memory and find large enough free size and store it
    let chosen_untyped_ix = bootinfo
        .untyped_list()
        .iter()
        .position(|desc| !desc.is_device() && desc.size_bits() >= blueprint.physical_size_bits())
        .unwrap();

    // Retrieve capability for that memory slot
    let untyped = bootinfo.untyped().index(chosen_untyped_ix).cap();

    // Find empty capability clots in the Cnode, then store in unbadged 
    // and baged notification slot to be assigned later
    let mut empty_slots = bootinfo
        .empty()
        .range()
        .map(sel4::init_thread::Slot::from_index);
    let unbadged_notification_slot = empty_slots.next().unwrap();
    let badged_notification_slot = empty_slots.next().unwrap();

    /* Retype the untyped capability into Notification object type in context of the 
    the current threads CNODE, use slot assigned to unbadged_notification */ 
    let cnode = sel4::init_thread::slot::CNODE.cap();

    untyped.untyped_retype(
        &blueprint,
        &cnode.relative_self(),
        unbadged_notification_slot.index(),
        1,
    )?;


    /*Derive another capability from the original unbaged notification capability s
    uch that the new capability only have write_only rights*/
    let badge = 0x1337;

    cnode.relative(badged_notification_slot.cptr()).mint(
        &cnode.relative(unbadged_notification_slot.cptr()),
        sel4::CapRights::write_only(),
        badge,
    )?;

    /*Finally, retrieve capabilites and make the badged notification signal,
    make the unbadged notification wait*/

    let unbadged_notification = unbadged_notification_slot
        .downcast::<sel4::cap_type::Notification>()
        .cap();
    let badged_notification = badged_notification_slot
        .downcast::<sel4::cap_type::Notification>()
        .cap();

    badged_notification.signal();

    let (_, observed_badge) = unbadged_notification.wait();

    //Check if observed badge is same as sent badge, if yes then task was success, quit!
    sel4::debug_println!("badge = {:#x}", badge);
    assert_eq!(observed_badge, badge);

    sel4::debug_println!("TEST_PASS");

    sel4::init_thread::suspend_self()
}
