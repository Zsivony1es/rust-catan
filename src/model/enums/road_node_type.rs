enum RoadNodeType {
    HasVP,
    HasVPWithSettler(PlayerColor),
    HasTown(PlayerColor),
    Empty,
}