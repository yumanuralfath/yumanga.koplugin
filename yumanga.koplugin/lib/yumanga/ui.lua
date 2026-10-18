local Device = require("device")
local DownloadMgr = require("ui/downloadmgr")
local InfoMessage = require("ui/widget/infomessage")
local InputDialog = require("ui/widget/inputdialog")
local MultiInputDialog = require("ui/widget/multiinputdialog")
local ProgressbarDialog = require("ui/widget/progressbardialog")
local Trapper = require("ui/trapper")
local UIManager = require("ui/uimanager")
local NetworkMgr = require("ui/network/manager")
local ffiUtil = require("ffi/util")
local _ = require("gettext")
local T = ffiUtil.template

local Core = require("lib/yumanga/core")
local Sources = require("lib/yumanga/sources")
local ChapterPicker = require("lib/yumanga/chapter_picker")
local DetailView = require("lib/yumanga/detail_view")
local ResultsView = require("lib/yumanga/results_view")
local Format = require("lib/yumanga/format")
local UI = {}

local function isSearchDetailsEnabled()
	local val = G_reader_settings:readSetting("yumanga_search_details")
	if val == nil then
		val = G_reader_settings:readSetting("weebcentral_search_details")
	end
	if val == nil then
		return true
	end
	return val == true
end

local function setSearchDetailsEnabled(enabled)
	G_reader_settings:saveSetting("yumanga_search_details", enabled)
end

local function isTurnOffWifiEnabled()
	if G_reader_settings:isTrue("yumanga_turn_off_wifi_after_download") then
		return true
	end
	return G_reader_settings:isTrue("weebcentral_turn_off_wifi_after_download")
end

local function setTurnOffWifiEnabled(enabled)
	G_reader_settings:saveSetting("yumanga_turn_off_wifi_after_download", enabled)
end

local function runWithProgress(plugin, message, command, progress_total, ...)
	local arguments = { ... }
	local progress
	if progress_total then
		progress = ProgressbarDialog:new({
			title = message,
			subtitle = _("Preparing download..."),
			progress_max = progress_total,
			refresh_time_seconds = 1,
		})
		progress:show()
	else
		progress = InfoMessage:new({
			text = message .. " (tap to cancel)",
		})
		UIManager:show(progress)
	end
	UIManager:forceRePaint()

	local stopped = false
	local poll_progress
	if progress_total then
		os.remove(plugin.progress_path)
		poll_progress = function()
			if stopped then
				return
			end
			local file = io.open(plugin.progress_path, "r")
			if file then
				local status = file:read("*l") or ""
				file:close()
				local stage, current, total = status:match("^(%a+)|(%d+)|(%d+)|")
				current, total = tonumber(current), tonumber(total)
				if current and total and total > 0 then
					local stage_fraction = math.min(current / total, 1)
					local fraction = stage == "scan" and stage_fraction * 0.1 or 0.1 + stage_fraction * 0.9
					progress:reportProgress(fraction * progress_total)
				end
			end
			UIManager:scheduleIn(0.5, poll_progress)
		end
		UIManager:scheduleIn(0.5, poll_progress)
	end

	local completed, output, succeeded = Trapper:dismissableRunInSubprocess(function()
		return Core.run(plugin.plugin_directory, command, unpack(arguments))
	end, progress)
	stopped = true
	if poll_progress then
		UIManager:unschedule(poll_progress)
	end
	os.remove(plugin.progress_path)
	if completed then
		if progress_total then
			progress:close()
		else
			UIManager:close(progress)
		end
	elseif progress_total then
		UIManager:close(progress)
	end
	if not completed then
		return nil, false
	end
	return output, succeeded
end

local function showMessage(message)
	UIManager:show(InfoMessage:new({ text = message }))
end

local function decodeCoreResponse(output, succeeded)
	return Core.decodeResponse(output, succeeded)
end

function UI.addToMainMenu(plugin, menu_items)
	if not plugin.ui.view then
		menu_items.yumanga = {
			text = _("YuManga Downloader"),
			sorting_hint = "search",
			sub_item_table = {
				{
					text = _("Search manga"),
					callback = function()
						UI.openSearchDialog(plugin)
					end,
				},
				{
					text = _("Refresh source catalog"),
					help_text = _("Refresh the Aidoku source catalog from the internet."),
					callback = function(touchmenu_instance)
						Sources.refresh(plugin, touchmenu_instance)
					end,
				},
				{
					text_func = function()
						return Sources.summary(plugin)
					end,
					help_text = _("Choose which sources are searched. New sources appear here automatically."),
					sub_item_table_func = function()
						return Sources.menuItems(plugin)
					end,
				},
				{
					text = _("Choose download folder"),
					callback = function()
						UI.chooseOutputDirectory(plugin)
					end,
				},
				{
					text = _("Show author, status and tags in results"),
					help_text = _("Loads extra details for each search result. Slower on weak Wi-Fi."),
					checked_func = function()
						return isSearchDetailsEnabled()
					end,
					callback = function()
						setSearchDetailsEnabled(not isSearchDetailsEnabled())
					end,
				},
				{
					text = _("Clear Cache"),
					callback = function()
						UI.clearCoverCache(plugin)
					end,
				},
				{
					text = _("Turn off Wi-Fi after download"),
					enabled_func = function()
						return Device:hasWifiToggle()
					end,
					checked_func = function()
						return isTurnOffWifiEnabled()
					end,
					callback = function()
						setTurnOffWifiEnabled(not isTurnOffWifiEnabled())
					end,
				},
			},
		}
	end
end

function UI.openSearchDialog(plugin)
	local dialog
	dialog = MultiInputDialog:new({
		title = _("Search manga"),
		description = T(_("Sources: %1"), Sources.names(plugin)),
		fields = {
			{
				text = plugin.search_keyword or "",
				hint = _("Manga title (optional)"),
			},
			{
				text = plugin.search_genre or "",
				hint = _("Optional genre, e.g. Comedy"),
				description = _("Matches genres provided by each source."),
			},
		},
		buttons = {
			{
				{
					text = _("Cancel"),
					id = "close",
					callback = function()
						UIManager:close(dialog)
					end,
				},
				{
					text = _("Search"),
					is_enter_default = true,
					callback = function()
						local fields = dialog:getFields()
						plugin.search_keyword = (fields[1] or ""):match("^%s*(.-)%s*$")
						plugin.search_genre = (fields[2] or ""):match("^%s*(.-)%s*$")
						UIManager:close(dialog)
						if plugin.search_keyword ~= "" or plugin.search_genre ~= "" then
							Trapper:wrap(function()
								UI.search(plugin, plugin.search_keyword, plugin.search_genre)
							end)
						end
					end,
				},
			},
		},
	})
	UIManager:show(dialog)
	dialog:onShowKeyboard()
end

function UI.search(plugin, keyword, genre)
	local selected = Sources.selected(plugin)
	if #selected == 0 then
		showMessage(_("No sources available. Check that the core executable is installed."))
		return
	end
	local with_details = isSearchDetailsEnabled() and "1" or "0"

	-- One source at a time (never in parallel) to keep memory use low.
	local combined, failures = {}, {}
	for index, source in ipairs(selected) do
		local label = source.name
		if #selected > 1 then
			label = string.format("%s (%d/%d)", source.name, index, #selected)
		end
		local output, succeeded = runWithProgress(
			plugin,
			T(_("Searching %1..."), label),
			"search",
			nil,
			source.id,
			keyword,
			plugin.cover_cache_directory,
			with_details,
			genre or ""
		)
		if output == nil then
			return -- cancelled
		end
		local results, err = decodeCoreResponse(output, succeeded)
		if results then
			for _i, result in ipairs(results) do
				result.source_id = source.id
				result.source_name = source.name
				combined[#combined + 1] = result
			end
		else
			failures[#failures + 1] = source.name .. ": " .. tostring(err)
		end
	end

	if #combined == 0 then
		if #failures > 0 then
			showMessage(table.concat(failures, "\n"))
		else
			showMessage(_("No manga found."))
		end
		return
	end
	UI.showSearchResults(plugin, combined, #selected > 1)
	if #failures > 0 then
		showMessage(_("Some sources failed:") .. "\n" .. table.concat(failures, "\n"))
	end
end

function UI.showSearchResults(plugin, results, show_source)
	if #results == 0 then
		showMessage(_("No manga found."))
		return
	end

	UIManager:show(
		ResultsView:new({
			results = results,
			show_source = show_source,
			on_select = function(result)
				-- The results stay open underneath, so closing the details comes back to them.
				Trapper:wrap(function()
					UI.loadMangaInfo(plugin, result)
				end)
			end,
		}),
		"ui"
	)
end

function UI.loadMangaInfo(plugin, series)
	local output, succeeded =
		runWithProgress(plugin, _("Loading info..."), "chapters", nil, series.source_id, series.id)
	local data, err = decodeCoreResponse(output, succeeded)
	if not data then
		if err then
			showMessage(err)
		end
		return
	end
	data.cover_path = series.cover_path
	data.source_id = series.source_id
	data.source_name = series.source_name
	-- Keep what the search already knew if the series page lacks a field.
	for _, field in ipairs({ "authors", "tags", "manga_type", "status", "released" }) do
		if (data[field] or "") == "" then
			data[field] = series[field] or ""
		end
	end
	UI.showMangaInfo(plugin, data)
end

function UI.showMangaInfo(plugin, series)
	local chapters = series.chapters or {}
	if #chapters == 0 then
		showMessage(_("No chapters found for ") .. series.title)
		return
	end

	UIManager:show(
		DetailView:new({
			series = series,
			on_choose = function()
				UI.openChapterPicker(plugin, series)
			end,
			on_enter_numbers = function()
				UI.openChapterSelection(plugin, series)
			end,
			on_quick = function(mode)
				UI.askQuickRange(plugin, series, mode)
			end,
			on_download = function(selection, count)
				UI.promptVolumeName(plugin, series, selection, count)
			end,
		}),
		"ui"
	)
end

function UI.openChapterPicker(plugin, series)
	UIManager:show(
		ChapterPicker:new({
			series = series,
			on_download = function(picker, selection, count)
				UI.promptVolumeName(plugin, series, selection, count, function(done)
					if done then
						picker:clearSelection()
					end
				end)
			end,
		}),
		"ui"
	)
end

-- Asks for an optional volume name, then downloads `selection`.
-- `on_finished(succeeded)` runs after the download attempt.
function UI.promptVolumeName(plugin, series, selection, count, on_finished)
	local dialog
	dialog = InputDialog:new({
		title = _("Download chapters"),
		input = plugin.last_volume_name or "",
		input_hint = _("Optional volume name, e.g. Vol 1"),
		description = string.format(
			_("%d chapter(s) selected.\nLeave the name blank to create one CBZ per chapter."),
			count
		),
		buttons = {
			{
				{
					text = _("Cancel"),
					id = "close",
					callback = function()
						UIManager:close(dialog)
					end,
				},
				{
					text = _("Download"),
					is_enter_default = true,
					callback = function()
						local volume_name = dialog:getInputText():match("^%s*(.-)%s*$")
						plugin.last_volume_name = volume_name
						UIManager:close(dialog)
						Trapper:wrap(function()
							local succeeded = UI.downloadSelection(plugin, series, selection, volume_name)
							if on_finished then
								on_finished(succeeded == true)
							end
						end)
					end,
				},
			},
		},
	})
	UIManager:show(dialog)
	dialog:onShowKeyboard()
end

-- Asks how many chapters, then downloads the first or the latest N.
-- `mode` is "first" (oldest chapters) or "latest" (newest chapters).
function UI.askQuickRange(plugin, series, mode)
	local total = #series.chapters
	local builder = mode == "first" and Format.first or Format.latest
	local dialog
	dialog = InputDialog:new({
		title = mode == "first" and _("Download first chapters") or _("Download latest chapters"),
		input = tostring(plugin.last_quick_count or 5),
		input_type = "number",
		input_hint = string.format(_("1 - %d"), total),
		description = mode == "first" and string.format(_("How many of the oldest chapters? (max %d)"), total)
			or string.format(_("How many of the newest chapters? (max %d)"), total),
		buttons = {
			{
				{
					text = _("Cancel"),
					id = "close",
					callback = function()
						UIManager:close(dialog)
					end,
				},
				{
					text = _("Next"),
					is_enter_default = true,
					callback = function()
						local input_text = dialog:getInputText():match("^%s*(.-)%s*$")
						local amount = tonumber(input_text)
						if not amount or amount < 1 then
							showMessage(_("Enter a number of chapters (1 or more)."))
							return
						end
						amount = math.min(math.floor(amount), total)
						plugin.last_quick_count = amount
						local selection, count = builder(total, amount)
						if not selection then
							return
						end
						UIManager:close(dialog)
						UI.promptVolumeName(plugin, series, selection, count)
					end,
				},
			},
		},
	})
	UIManager:show(dialog)
	dialog:onShowKeyboard()
end

function UI.openChapterSelection(plugin, series)
	local dialog
	dialog = MultiInputDialog:new({
		title = _("Select chapters to download"),
		fields = {
			{
				text = plugin.last_selection or "",
				hint = _("1,3,4 or 1-10"),
				description = _("Chapter numbers: ") .. #series.chapters,
			},
			{
				text = plugin.last_volume_name or "",
				hint = _("Optional volume name, e.g. Vol 1"),
				description = _("Leave blank to create one CBZ per chapter."),
			},
		},
		buttons = {
			{
				{
					text = _("Cancel"),
					id = "close",
					callback = function()
						UIManager:close(dialog)
					end,
				},
				{
					text = _("Download"),
					is_enter_default = true,
					callback = function()
						local fields = dialog:getFields()
						local selection = (fields[1] or ""):match("^%s*(.-)%s*$")
						local volume_name = (fields[2] or ""):match("^%s*(.-)%s*$")
						if selection == "" then
							showMessage(_("Enter chapter numbers to download."))
							return
						end
						plugin.last_selection = selection
						plugin.last_volume_name = volume_name
						UIManager:close(dialog)
						Trapper:wrap(function()
							UI.downloadSelection(plugin, series, selection, volume_name)
						end)
					end,
				},
			},
		},
	})
	UIManager:show(dialog)
	dialog:onShowKeyboard()
end

function UI.downloadSelection(plugin, series, selection, volume_name)
	local selection_count = UI.countSelectedChapters(selection, #series.chapters)
	if not selection_count then
		showMessage(_("Check the chapter selection and try again."))
		return false
	end
	local output, succeeded = runWithProgress(
		plugin,
		_("Downloading selected chapters..."),
		"download",
		1000,
		series.source_id,
		series.id,
		selection,
		volume_name,
		plugin.output_directory,
		plugin.progress_path
	)
	local result, err = decodeCoreResponse(output, succeeded)
	if not result then
		if err then
			showMessage(err)
		end
		return false
	end

	local message = _("Download complete") .. "\n\n" .. table.concat(result.paths or {}, "\n")
	if (result.skipped or 0) > 0 then
		message = message .. "\n" .. _("Existing files skipped: ") .. result.skipped
	end
	showMessage(message)
	if Device:hasWifiToggle() and isTurnOffWifiEnabled() then
		UIManager:nextTick(function()
			NetworkMgr:disableWifi()
		end)
	end
	return true
end

function UI.countSelectedChapters(selection, chapter_count)
	local selected = {}
	for part in selection:gmatch("[^,]+") do
		local first, last = part:match("^%s*(%d+)%s*%-%s*(%d+)%s*$")
		if first then
			first, last = tonumber(first), tonumber(last)
			if first < 1 or last < first or last > chapter_count then
				return nil
			end
			for index = first, last do
				selected[index] = true
			end
		else
			local chapter = tonumber(part:match("^%s*(%d+)%s*$"))
			if not chapter or chapter < 1 or chapter > chapter_count then
				return nil
			end
			selected[chapter] = true
		end
	end
	local count = 0
	for _ in pairs(selected) do
		count = count + 1
	end
	return count > 0 and count or nil
end

function UI.chooseOutputDirectory(plugin)
	DownloadMgr:new({
		title = _("Choose download folder"),
		onConfirm = function(path)
			plugin.output_directory = path
			G_reader_settings:saveSetting("yumanga_output_directory", path)
		end,
	}):chooseDir(plugin.output_directory)
end

function UI.clearCoverCache(plugin)
	ffiUtil.purgeDir(plugin.cover_cache_directory)
	showMessage("Cache Cleared")
end

return UI
