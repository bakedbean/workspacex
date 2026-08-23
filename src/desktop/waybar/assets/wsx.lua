-- wsx workspace picker for elephant/walker.
-- Installed by `wsx setup waybar`; edits are overwritten on re-run.
-- Launch with: walker -m menus:wsx
Name = "wsx"
NamePretty = "wsx Workspaces"
HideFromProviderlist = true

-- Shell-quoted absolute path, substituted at install time. Long-string
-- brackets so the single-quoted substitution lands verbatim.
local WSX = [[__WSX_BIN__]]

function GetEntries()
  local entries = {}
  local handle = io.popen(WSX .. " waybar menu-entries --json 2>/dev/null")
  if not handle then
    return entries
  end
  local out = handle:read("*a")
  handle:close()
  if not out or out == "" then
    return entries
  end
  local decoded = jsonDecode(out)
  if type(decoded) ~= "table" then
    return entries
  end
  for _, e in ipairs(decoded) do
    if type(e) == "table" and e.text and e.action then
      local entry = {
        Text = e.text,
        Subtext = e.subtext,
        Icon = e.icon,
        Actions = { activate = e.action },
      }
      -- Row CSS classes (walker styles them via the wsx theme).
      if type(e.state) == "table" and #e.state > 0 then
        entry.State = e.state
      end
      table.insert(entries, entry)
    end
  end
  return entries
end
