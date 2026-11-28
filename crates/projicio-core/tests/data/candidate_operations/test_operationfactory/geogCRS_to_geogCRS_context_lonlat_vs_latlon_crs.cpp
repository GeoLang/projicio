TEST(operation, geogCRS_to_geogCRS_context_lonlat_vs_latlon_crs) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    auto ctxt = CoordinateOperationContext::create(authFactory, nullptr, 0.0);
    ctxt->setGridAvailabilityUse(
        CoordinateOperationContext::GridAvailabilityUse::
            IGNORE_GRID_AVAILABILITY);
    ctxt->setSpatialCriterion(
        CoordinateOperationContext::SpatialCriterion::PARTIAL_INTERSECTION);
    auto list = CoordinateOperationFactory::create()->createOperations(
        authFactory->createCoordinateReferenceSystem("4749"),  // RGNC91-93
        authFactory->createCoordinateReferenceSystem("10310"), // RGNC15
        ctxt);
    ASSERT_EQ(list.size(), 5U);
    EXPECT_EQ(list[0]->nameStr(),
              "RGNC91-93 to WGS 84 (1) + Inverse of RGNC15 to WGS 84 (1)");
    // Check that we get direct transformation, and not only through WGS 84
    // The difficulty here is that the transformation is registered between
    // "RGNC91-93 (lon-lat)" et "RGNC15 (lon-lat)"
    EXPECT_EQ(list[1]->nameStr(), "axis order change (2D) + RGNC91-93 to "
                                  "RGNC15 (2) + axis order change (2D)");
    EXPECT_EQ(list[2]->nameStr(), "axis order change (2D) + RGNC91-93 to "
                                  "RGNC15 (1) + axis order change (2D)");
}

