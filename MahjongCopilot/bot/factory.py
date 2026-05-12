""" Bot factory"""
from common.settings import Settings
from common.utils import Folder, sub_file
from .bot import Bot, GameMode
from .local.bot_local import BotMortalLocal


MODEL_TYPE_STRINGS = ["Local"]


def get_bot(settings:Settings) -> Bot:
    """ create the Bot instance based on settings"""
    model_files:dict = {
        GameMode.MJ4P: sub_file(Folder.MODEL, settings.model_file),
        GameMode.MJ3P: sub_file(Folder.MODEL, settings.model_file_3p)
    }
    return BotMortalLocal(model_files)


