const std = @import("std");

pub fn main() void {
    const v = my_func(1, 2);
    std.debug.print("Hello, world!{d}\n", .{v});
}

comptime {
    asm (
        \\.global my_func;
        \\.type my_func, @function;
        \\my_func:
        \\  addw a0, a0, a1
        \\  ret
    );
}

extern fn my_func(a: i32, b: i32) i32;
