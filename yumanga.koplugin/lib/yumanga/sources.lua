-- Source selection: asks the core which sources exist, remembers which ones
-- are enabled, and warns when too many are enabled for a small e-reader.
local ConfirmBox = require("ui/widget/confirmbox")
local InfoMessage = require("ui/widget/infomessage")
local UIManager = require("ui/uimanager")
local T = require("ffi/util").template
local _ = require("gettext")

local Core = require("lib/yumanga/core")

local Sources = {}

-- Searching runs one source after another, but every extra source still adds
-- results, covers and detail requests. Above this, the user gets a warning.
Sources.MAX_RECOMMENDED = 3

local SETTING = "yumanga_selected_sources"

-- Sources known to the core executable: { { id = "...", name = "..." }, ... }.
-- Cached per session; `refresh` reloads the online Aidoku catalog.
function Sources.available(plugin, refresh)
	if plugin.sources and not refresh then
		return plugin.sources
	end
	local output, succeeded = Core.run(plugin.plugin_directory, "sources")
	local data, err = Core.decodeResponse(output, succeeded)
	if not data then
		plugin.sources_error = err
		return {} -- not cached, so the next call tries again
	end
	plugin.sources_error = nil
	plugin.sources = data
	return data
end

local function isAidoku(source)
	return source.id:match("^[%w%-]+%.") ~= nil
end

function Sources.refresh(plugin, touchmenu_instance)
	plugin.sources = nil
	local available = Sources.available(plugin, true)
	if #available == 0 and plugin.sources_error then
		UIManager:show(InfoMessage:new({
			text = plugin.sources_error,
			timeout = 6,
		}))
		return
	end
	local aidoku_count = 0
	for _i, source in ipairs(available) do
		if isAidoku(source) then
			aidoku_count = aidoku_count + 1
		end
	end
	UIManager:show(InfoMessage:new({
		text = T(_("Loaded %1 sources, including %2 Aidoku sources."), #available, aidoku_count),
		timeout = 4,
	}))
	if touchmenu_instance then
		touchmenu_instance:updateItems()
	end
end

-- Enabled sources, in the core's order. Falls back to the first source when
-- nothing valid is stored (first run, or a stored source no longer exists).
function Sources.selected(plugin)
	local available = Sources.available(plugin)
	local wanted = {}
	local saved = G_reader_settings:readSetting(SETTING)
		or G_reader_settings:readSetting("weebcentral_selected_sources")
		or {}
	for _i, id in ipairs(saved) do
		wanted[id] = true
	end
	local picked = {}
	for _i, source in ipairs(available) do
		if wanted[source.id] then
			picked[#picked + 1] = source
		end
	end
	if #picked == 0 and #available > 0 then
		picked[1] = available[1]
	end
	return picked
end

function Sources.isSelected(plugin, id)
	for _i, source in ipairs(Sources.selected(plugin)) do
		if source.id == id then
			return true
		end
	end
	return false
end

-- "WeebCentral, Other" for dialogs.
function Sources.names(plugin)
	local names = {}
	for _i, source in ipairs(Sources.selected(plugin)) do
		names[#names + 1] = source.name
	end
	return table.concat(names, ", ")
end

local function save(ids)
	G_reader_settings:saveSetting(SETTING, ids)
end

-- Enables or disables one source. Keeps at least one enabled and asks for
-- confirmation when enabling would pass MAX_RECOMMENDED.
function Sources.toggle(plugin, source, touchmenu_instance)
	local ids, was_selected = {}, false
	for _i, current in ipairs(Sources.selected(plugin)) do
		if current.id == source.id then
			was_selected = true
		else
			ids[#ids + 1] = current.id
		end
	end

	local function refresh()
		if touchmenu_instance then
			touchmenu_instance:updateItems()
		end
	end

	if was_selected then
		if #ids == 0 then
			UIManager:show(InfoMessage:new({
				text = _("At least one source must stay selected."),
				timeout = 3,
			}))
			return
		end
		save(ids)
		refresh()
		return
	end

	ids[#ids + 1] = source.id
	if #ids > Sources.MAX_RECOMMENDED then
		UIManager:show(ConfirmBox:new({
			text = T(
				_(
					"You are about to search %1 sources at once (recommended: %2 or fewer).\n\nE-readers have little memory, so many sources can make searching slow or freeze the device. Turning off \"Show author, status and tags in results\" helps.\n\nUse it anyway?"
				),
				#ids,
				Sources.MAX_RECOMMENDED
			),
			ok_text = _("Use anyway"),
			cancel_text = _("Cancel"),
			ok_callback = function()
				save(ids)
				refresh()
			end,
		}))
		return
	end
	save(ids)
	refresh()
end

-- Menu entries, one checkbox per source. Built every time the submenu opens,
-- so a source added to the core shows up without any change to the UI.
function Sources.menuItems(plugin)
	local available = Sources.available(plugin, true)
	if #available == 0 then
		return { { text = _("No sources found"), enabled = false } }
	end
	local items = {}
	for _i, source in ipairs(available) do
		local label = source.name
		if isAidoku(source) then
			label = T(_("Aidoku: %1"), label)
		end
		items[#items + 1] = {
			text = label,
			checked_func = function()
				return Sources.isSelected(plugin, source.id)
			end,
			keep_menu_open = true,
			callback = function(touchmenu_instance)
				Sources.toggle(plugin, source, touchmenu_instance)
			end,
		}
	end
	return items
end

-- "Sources (1/3)" for the parent menu entry.
function Sources.summary(plugin)
	local available = Sources.available(plugin)
	local aidoku_count = 0
	for _i, source in ipairs(available) do
		if isAidoku(source) then
			aidoku_count = aidoku_count + 1
		end
	end
	return T(_("Sources (%1/%2, %3 Aidoku)"), #Sources.selected(plugin), #available, aidoku_count)
end

return Sources
