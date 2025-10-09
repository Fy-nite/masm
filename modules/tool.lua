return {
  MNI_MODULE = "tool",
  MNI_FUNCTIONS = {
    set_rax = function(args, regs)
      print("tool.set_rax called with args:", table.concat(args, ", "))
      local v = tonumber(args[1] or "0") or 0
      return { regs = { RAX = v } }
    end,
    echo = function(args, regs)
      return table.concat(args, " ")
    end
  }
}
