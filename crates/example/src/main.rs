#![no_std]
#![no_main]

// Same initialization as example
extern crate alloc;

use core::ptr;
use object::{File, Object};
use sel4_root_task::{root_task, Never};

mod child_vspace;
mod object_allocator;

use child_vspace::create_child_vspace;
use object_allocator::ObjectAllocator;

// Same initialization as example, loading children from executable and linkable binary file
const CHILD1_ELF_CONTENTS: &[u8] = include_bytes!(env!("CHILD1_ELF"));
const CHILD2_ELF_CONTENTS: &[u8] = include_bytes!(env!("CHILD2_ELF")); // <-- where this?

#[root_task(heap_size = 1024 * 64)]
fn main(bootinfo: &sel4::BootInfoPtr) -> sel4::Result<Never> {
    sel4::debug_println!("In root task");

    // Allocates Kernel Objects (e.g TCB, frames etc.)
    let mut object_allocator = ObjectAllocator::new(bootinfo);

    // Gives free address page for process to use
    let free_page_addr1 = init_free_page_addr(bootinfo);
    let free_page_addr2 = init_free_page_addr(bootinfo);

    // Parses the ELF files
    let client_image = File::parse(CHILD1_ELF_CONTENTS).unwrap();
    let server_image = File::parse(CHILD2_ELF_CONTENTS).unwrap();

    // Assigns a virtual address space to the child, 
    let (child1_vspace, ipc_buffer_addr1, ipc_buffer_cap1) = create_child_vspace(
        &mut object_allocator,
        &client_image,
        sel4::init_thread::slot::VSPACE.cap(),
        free_page_addr1,
        sel4::init_thread::slot::ASID_POOL.cap(),
    );

    /*Assigns a virtual address space to the child, gets ipc_buffer address to allow 
    both processes to communicate and ipc_buffer_cap for capability */ 
    let (child2_vspace, ipc_buffer_addr2, ipc_buffer_cap2) = create_child_vspace(
        &mut object_allocator,
        &server_image,
        sel4::init_thread::slot::VSPACE.cap(),
        free_page_addr2,
        sel4::init_thread::slot::ASID_POOL.cap(),
    );

    // Creates a notification capability for the root process
    let client_to_server_nfn = object_allocator.allocate_fixed_sized::<sel4::cap_type::Notification>();

    // Set number of bits to allocate to Cnode of each process
    let child_cnode_size_bits = 2;

    // Allocate Cnode space and mint a capabilty at address space 2 for a notification capability with write only rights
    let client_cnode = object_allocator.allocate_variable_sized::<sel4::cap_type::CNode>(child_cnode_size_bits);
    client_cnode
        .relative_bits_with_depth(1, child_cnode_size_bits) // figure this out
        .mint(
            &sel4::init_thread::slot::CNODE
                .cap()
                .relative(client_to_server_nfn),
            sel4::CapRights::write_only(),
            0,
        )
        .unwrap();

    // Allocate Cnode space and mint a capabilty at address space 2 for a notification capability with write only rights
    let server_cnode = object_allocator.allocate_variable_sized::<sel4::cap_type::CNode>(child_cnode_size_bits);
    server_cnode
        .relative_bits_with_depth(1, child_cnode_size_bits) // does this need to change (also allocating to same space?)
        .mint(
            &sel4::init_thread::slot::CNODE
                .cap()
                .relative(child1_to_child2_nfn),
            sel4::CapRights::read_only(),
            0,
        )
        .unwrap();

    // Creating a thread capability
    let client_tcb = object_allocator.allocate_fixed_sized::<sel4::cap_type::Tcb>();
    client_tcb
        .tcb_configure(
            sel4::init_thread::slot::NULL.cptr(),
            child1_cnode,
            sel4::CNodeCapData::new(0, sel4::WORD_SIZE - child_cnode_size_bits),
            child1_vspace,
            ipc_buffer_addr1 as sel4::Word,
            ipc_buffer_cap1,
        )
        .unwrap();

    // Creating a thread capability
    let server_tcb = object_allocator.allocate_fixed_sized::<sel4::cap_type::Tcb>();
    server_tcb
        .tcb_configure(
            sel4::init_thread::slot::NULL.cptr(),
            child2_cnode,
            sel4::CNodeCapData::new(0, sel4::WORD_SIZE - child_cnode_size_bits),
            child2_vspace,
            ipc_buffer_addr2 as sel4::Word,
            ipc_buffer_cap2,
        )
        .unwrap();

    // Giving child process full access to its TCB capability 
    client_cnode
        .relative_bits_with_depth(2, child_cnode_size_bits)
        .mint(
            &sel4::init_thread::slot::CNODE.cap().relative(child1_tcb),
            sel4::CapRights::all(),
            0,
        )
        .unwrap();

    // Giving child process full access to its TCB capability
    server_cnode
        .relative_bits_with_depth(2, child_cnode_size_bits)
        .mint(
            &sel4::init_thread::slot::CNODE.cap().relative(child2_tcb),
            sel4::CapRights::all(),
            0,
        )
        .unwrap();

    // Set up and launch both child processes
    let mut ctx1 = sel4::UserContext::default();
    *ctx1.pc_mut() = client_image.entry().try_into().unwrap();
    client_tcb.tcb_write_all_registers(true, &mut ctx1).unwrap();

    let mut ctx2 = sel4::UserContext::default();
    *ctx2.pc_mut() = server_image.entry().try_into().unwrap();
    server_tcb.tcb_write_all_registers(true, &mut ctx2).unwrap();

    sel4::debug_println!("Server and Client talk to each other (Inshallah)!");

    sel4::init_thread::suspend_self()
}

// // //

#[repr(C, align(4096))]
struct FreePagePlaceHolder(#[allow(dead_code)] [u8; GRANULE_SIZE]);

static mut FREE_PAGE_PLACEHOLDER: FreePagePlaceHolder = FreePagePlaceHolder([0; GRANULE_SIZE]);

fn init_free_page_addr(bootinfo: &sel4::BootInfo) -> usize {
    let addr = ptr::addr_of!(FREE_PAGE_PLACEHOLDER) as usize;
    get_user_image_frame_slot(bootinfo, addr)
        .cap()
        .frame_unmap()
        .unwrap();
    addr
}

fn get_user_image_frame_slot(
    bootinfo: &sel4::BootInfo,
    addr: usize,
) -> sel4::init_thread::Slot<sel4::cap_type::Granule> {
    extern "C" {
        static __executable_start: usize;
    }
    let user_image_addr = ptr::addr_of!(__executable_start) as usize;
    bootinfo
        .user_image_frames()
        .index(addr / GRANULE_SIZE - user_image_addr / GRANULE_SIZE)
}

const GRANULE_SIZE: usize = sel4::FrameObjectType::GRANULE.bytes();
