-- User commands for linear-tui.nvim. Loading is lazy: the modules are only
-- required when a command actually runs.
if vim.g.loaded_linear_tui then
  return
end
vim.g.loaded_linear_tui = true

vim.api.nvim_create_user_command("Linear", function()
  require("linear-tui").open()
end, { desc = "Open the linear-tui UI" })

vim.api.nvim_create_user_command("LinearToggle", function()
  require("linear-tui").toggle()
end, { desc = "Toggle the linear-tui UI" })

vim.api.nvim_create_user_command("LinearClose", function()
  require("linear-tui").close()
end, { desc = "Close the linear-tui UI" })
